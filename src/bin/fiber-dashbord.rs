use std::{
    sync::{
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    vec,
};

use ckb_jsonrpc_types::JsonBytes;
use fiber_dashbord_backend::{
    CHANNEL_MONITOR_HEARTBEAT, RpcClient,
    clock_timer::ClockTimer,
    create_pg_pool, get_pg_pool, init_db,
    pg_write::{
        ChannelInfoDBSchema, channel_states_monitor, daily_statistics, from_rpc_to_db_schema,
        init_global_cache, insert_batch,
    },
    types::{GraphChannelsParams, GraphNodesParams},
};

use reqwest::Url;
use sqlx::{Row, types::chrono::Utc};

fn open_fds() -> Option<usize> {
    match std::fs::read_dir("/proc/self/fd") {
        // read_dir's own directory fd is listed inside /proc/self/fd.
        Ok(entries) => Some(entries.filter_map(Result::ok).count().saturating_sub(1)),
        Err(err) => {
            log::warn!("Failed to read /proc/self/fd: {}", err);
            None
        }
    }
}

fn fd_limits() -> (Option<u64>, Option<u64>) {
    let Ok(limits) = std::fs::read_to_string("/proc/self/limits") else {
        return (None, None);
    };
    let Some(line) = limits
        .lines()
        .find(|line| line.starts_with("Max open files"))
    else {
        return (None, None);
    };
    let mut values = line.split_whitespace().skip(3);
    let soft = values.next().and_then(|value| value.parse().ok());
    let hard = values.next().and_then(|value| value.parse().ok());
    (soft, hard)
}

fn main() {
    env_logger::init();
    if std::env::var("ALLOW_EXIT_ON_PANIC")
        .unwrap_or_default()
        .parse()
        .unwrap_or(true)
    {
        std::panic::set_hook(Box::new(|info| {
            let payload = info.payload();
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("<non-string panic payload>");
            let location = info
                .location()
                .map(|location| {
                    format!(
                        "{}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    )
                })
                .unwrap_or_else(|| "<unknown location>".to_string());
            log::error!("Panic occurred at {}: {}", location, message);
            std::process::exit(1);
        }));
    }

    let rt = tokio::runtime::Runtime::new().unwrap();

    rt.block_on(async move {
        create_pg_pool().await;
        let pool = get_pg_pool();
        init_db(pool).await;
        init_global_cache(pool).await;
        tokio::spawn(daily_commit());
        tokio::spawn(timed_commit_states());
        tokio::spawn(hourly_fresh());

        http_server().await;
    });
}

async fn http_server() {
    use fiber_dashbord_backend::http_server::{
        all_region, analysis, analysis_hourly, channel_by_state, channel_capacity_distribution,
        channel_count_by_asset, channel_count_by_state, channel_info, channel_state,
        channels_by_node_id, list_channels_hourly, list_channels_monthly, list_nodes_hourly,
        list_nodes_monthly, node_info, node_udt_infos, nodes_by_region, nodes_by_udt,
        nodes_fuzzy_by_name_or_id,
    };
    use salvo::{
        Depot, Listener, Request, Response, Router, Server, Service, conn::TcpListener,
        cors::AllowOrigin, cors::Cors, handler,
    };

    #[handler]
    pub async fn health_check(
        _req: &mut Request,
        _depot: &mut Depot,
        _res: &mut Response,
    ) -> Result<String, salvo::Error> {
        let timed_commit_states_heartbeat = TIMED_COMMIT_STATES_HEARTBEAT.load(Ordering::Acquire);
        let daily_commit_task_heartbeat = DAILY_COMMIT_TASK_HEARTBEAT.load(Ordering::Acquire);
        let hourly_fresh_task_heartbeat = HOURLY_FRESH_TASK_HEARTBEAT.load(Ordering::Acquire);
        let channel_monitor_heartbeat = CHANNEL_MONITOR_HEARTBEAT.load(Ordering::Acquire);
        let (open_fd_count, (fd_soft_limit, fd_hard_limit)) =
            tokio::task::spawn_blocking(|| (open_fds(), fd_limits()))
                .await
                .unwrap_or((None, (None, None)));

        Ok(serde_json::to_string(&serde_json::json!({
            "timed_commit_states_heartbeat": timed_commit_states_heartbeat,
            "daily_commit_task_heartbeat": daily_commit_task_heartbeat,
            "hourly_fresh_task_heartbeat": hourly_fresh_task_heartbeat,
            "channel_monitor_heartbeat": channel_monitor_heartbeat,
            "open_fds": open_fd_count,
            "fd_soft_limit": fd_soft_limit,
            "fd_hard_limit": fd_hard_limit,
        }))
        .unwrap())
    }

    use salvo::http::Method;
    let cors = Cors::new()
        .allow_origin(AllowOrigin::any())
        .allow_headers(vec!["content-type", "accept", "authorization"])
        .allow_methods(vec![Method::GET, Method::POST, Method::OPTIONS])
        .into_handler();
    let router = Router::new()
        .push(Router::with_path("nodes_hourly").get(list_nodes_hourly))
        .push(Router::with_path("channels_hourly").get(list_channels_hourly))
        .push(Router::with_path("node_udt_infos").get(node_udt_infos))
        .push(Router::with_path("nodes_by_udt").post(nodes_by_udt))
        .push(Router::with_path("nodes_nearly_monthly").get(list_nodes_monthly))
        .push(Router::with_path("channels_nearly_monthly").get(list_channels_monthly))
        .push(Router::with_path("analysis_hourly").get(analysis_hourly))
        .push(Router::with_path("analysis").post(analysis))
        .push(Router::with_path("channel_state").get(channel_state))
        .push(Router::with_path("group_channel_by_state").get(channel_by_state))
        .push(Router::with_path("channel_count_by_state").get(channel_count_by_state))
        .push(Router::with_path("channel_count_by_asset").get(channel_count_by_asset))
        .push(Router::with_path("channel_info").get(channel_info))
        .push(Router::with_path("node_info").get(node_info))
        .push(Router::with_path("channels_by_node_id").get(channels_by_node_id))
        .push(Router::with_path("nodes_by_region").get(nodes_by_region))
        .push(Router::with_path("nodes_fuzzy_by_name").get(nodes_fuzzy_by_name_or_id))
        .push(Router::with_path("all_region").get(all_region))
        .push(Router::with_path("channel_capacity_distribution").get(channel_capacity_distribution))
        .push(Router::with_path("health_check").get(health_check));

    let service = Service::new(router).hoop(cors);
    let http_port = std::env::var("HTTP_PORT").unwrap_or("8000".to_string());
    let listener = TcpListener::new(format!("0.0.0.0:{}", http_port))
        .bind()
        .await;
    let (fd_soft_limit, fd_hard_limit) = fd_limits();
    log::info!(
        "Open file limits (soft/hard): {:?}/{:?}",
        fd_soft_limit,
        fd_hard_limit
    );
    log::info!("Starting HTTP server on port {}", http_port);
    Server::new(listener).serve(service).await;
}

static MAINNET_FIBER_RPC_URL: LazyLock<Option<Url>> = LazyLock::new(|| {
    let url = std::env::var("FIBER_MAINNET_RPC_URL")
        .map(|url| Url::parse(&url).ok())
        .ok()
        .flatten();
    if url.is_none() {
        log::warn!("FIBER_MAINNET_RPC_URL is not set, mainnet fiber dashbord will be disabled");
    }

    url
});
static MAINNET_FIBER_RPC_BEARER_TOKEN: LazyLock<Option<String>> =
    LazyLock::new(|| std::env::var("FIBER_MAINNET_RPC_BEARER_TOKEN").ok());
static TESTNET_FIBER_RPC_URL: LazyLock<Option<Url>> = LazyLock::new(|| {
    let url = std::env::var("FIBER_TESTNET_RPC_URL")
        .map(|url| Url::parse(&url).ok())
        .ok()
        .flatten();
    if url.is_none() {
        log::warn!("FIBER_TESTNET_RPC_URL is not set, testnet fiber dashbord will be disabled");
    }

    url
});
static TESTNET_FIBER_RPC_BEARER_TOKEN: LazyLock<Option<String>> =
    LazyLock::new(|| std::env::var("FIBER_TESTNET_RPC_BEARER_TOKEN").ok());

static NETS: LazyLock<Vec<fiber_dashbord_backend::Network>> = LazyLock::new(|| {
    MAINNET_FIBER_RPC_URL
        .as_ref()
        .map(|_| fiber_dashbord_backend::Network::Mainnet)
        .into_iter()
        .chain(
            TESTNET_FIBER_RPC_URL
                .as_ref()
                .map(|_| fiber_dashbord_backend::Network::Testnet),
        )
        .collect::<Vec<_>>()
});

static TIMED_COMMIT_STATES_HEARTBEAT: AtomicU64 = AtomicU64::new(0);
const MAX_GRAPH_FETCH_ATTEMPTS: u32 = 5;

async fn timed_commit_states() {
    let mut rpc = RpcClient::new();
    let (tx, rx) = tokio::sync::mpsc::channel(8);

    tokio::spawn(channel_states_monitor(rpc.clone(), rx));
    let (mut testnet_init, mut mainnet_init) = (false, false);

    let mut heartbeat_timer = tokio::time::interval(tokio::time::Duration::from_secs(60));
    heartbeat_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut timed_timer = tokio::time::interval(tokio::time::Duration::from_secs(60 * 30));
    timed_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
                _ = heartbeat_timer.tick() => {
                    let timestamp = Utc::now().timestamp() as u64;
                    TIMED_COMMIT_STATES_HEARTBEAT.store(timestamp, Ordering::Release);
                }
                _ = timed_timer.tick() => {
                     timed_commit_states_inner(&mut rpc, &tx, &mut mainnet_init, &mut testnet_init).await;
            }
        }
    }
}

async fn timed_commit_states_inner(
    rpc: &mut RpcClient,
    tx: &tokio::sync::mpsc::Sender<(fiber_dashbord_backend::Network, Vec<JsonBytes>)>,
    mainnet_init: &mut bool,
    testnet_init: &mut bool,
) {
    for net in NETS.iter() {
        let url = match net {
            fiber_dashbord_backend::Network::Mainnet => {
                rpc.set_bearer_token(MAINNET_FIBER_RPC_BEARER_TOKEN.clone());
                MAINNET_FIBER_RPC_URL.clone().unwrap()
            }
            fiber_dashbord_backend::Network::Testnet => {
                rpc.set_bearer_token(TESTNET_FIBER_RPC_BEARER_TOKEN.clone());
                TESTNET_FIBER_RPC_URL.clone().unwrap()
            }
        };

        let mut raw_nodes = Vec::new();
        let mut after_cursor = None;
        let mut attempts = 0;
        let mut nodes_complete = true;

        loop {
            match rpc
                .get_node_graph(
                    url.clone(),
                    GraphNodesParams {
                        limit: None,
                        after: after_cursor.clone(),
                    },
                )
                .await
            {
                Ok(nodes) => {
                    attempts = 0;
                    let has_more = nodes.nodes.len() == 500;
                    raw_nodes.extend(nodes.nodes);

                    if !has_more {
                        break;
                    }

                    after_cursor = Some(nodes.last_cursor);
                }
                Err(err) => {
                    attempts += 1;
                    log::warn!(
                        "Failed to get {:?}'s node graph (attempt {}/{}): {}",
                        net,
                        attempts,
                        MAX_GRAPH_FETCH_ATTEMPTS,
                        err
                    );
                    if attempts >= MAX_GRAPH_FETCH_ATTEMPTS {
                        log::error!(
                            "Giving up on {:?}'s node graph after {} attempts",
                            net,
                            attempts
                        );
                        nodes_complete = false;
                        break;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }

        if !nodes_complete {
            log::error!("Discarding incomplete {:?} node graph", net);
            raw_nodes.clear();
        }

        let mut raw_channels = Vec::new();
        let mut after_cursor = None;
        let mut attempts = 0;
        let mut channels_complete = true;

        loop {
            match rpc
                .get_channel_graph(
                    url.clone(),
                    GraphChannelsParams {
                        limit: None,
                        after: after_cursor.clone(),
                    },
                )
                .await
            {
                Ok(channels) => {
                    attempts = 0;
                    let has_more = channels.channels.len() == 500;
                    raw_channels.extend(channels.channels);

                    if !has_more {
                        break;
                    }

                    after_cursor = Some(channels.last_cursor);
                }
                Err(err) => {
                    attempts += 1;
                    log::warn!(
                        "Failed to get {:?}'s channel graph (attempt {}/{}): {}",
                        net,
                        attempts,
                        MAX_GRAPH_FETCH_ATTEMPTS,
                        err
                    );
                    if attempts >= MAX_GRAPH_FETCH_ATTEMPTS {
                        log::error!(
                            "Giving up on {:?}'s channel graph after {} attempts",
                            net,
                            attempts
                        );
                        channels_complete = false;
                        break;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }

        if !channels_complete {
            log::error!("Discarding incomplete {:?} channel graph", net);
            raw_channels.clear();
        }

        if raw_nodes.is_empty() && raw_channels.is_empty() {
            log::error!("No {:?} graph data fetched this cycle, skipping insert", net);
            continue;
        }

        let mut node_schemas = Vec::with_capacity(raw_nodes.len());
        let mut udt_infos = Vec::new();
        let mut udt_dep_relations = Vec::new();
        let mut udt_node_relations = Vec::new();
        for node in raw_nodes {
            let (node_schema, udt_info, udt_dep_relation, udt_node_relation) =
                from_rpc_to_db_schema(node, *net).await;
            node_schemas.push(node_schema);
            udt_infos.extend(udt_info);
            udt_dep_relations.extend(udt_dep_relation);
            udt_node_relations.extend(udt_node_relation);
        }

        let mut channel_schemas = Vec::with_capacity(raw_channels.len());
        tx.send((
            *net,
            raw_channels
                .iter()
                .map(|c| c.channel_outpoint.clone())
                .collect::<Vec<_>>(),
        ))
        .await
        .expect("Failed to send channel outpoints to monitor");
        for channel in raw_channels {
            let channel_schema: ChannelInfoDBSchema = (channel, *net).into();
            channel_schemas.push(channel_schema);
        }

        log::info!(
            "{:?} Fetched {} nodes and {} channels",
            net,
            node_schemas.len(),
            channel_schemas.len()
        );

        let now = Utc::now();

        let pool = get_pg_pool();
        insert_batch(
            pool,
            &udt_infos,
            &udt_dep_relations,
            &udt_node_relations,
            &node_schemas,
            &channel_schemas,
            &now,
            *net,
        )
        .await
        .expect("Failed to insert batch");
        if match net {
            fiber_dashbord_backend::Network::Mainnet => !*mainnet_init,
            fiber_dashbord_backend::Network::Testnet => !*testnet_init,
        } {
            let sql = format!("SELECT COUNT(*) FROM {}", net.online_nodes_hourly());
            let count = sqlx::query(&sql)
                .fetch_one(pool)
                .await
                .map(|row| row.get::<i64, _>(0))
                .expect("Failed to count rows");
            if count == 0 {
                let flush_nodes_sql = format!(
                    "CALL refresh_continuous_aggregate('{}', NULL, NULL)",
                    net.online_nodes_hourly()
                );
                let flush_channels_sql = format!(
                    "CALL refresh_continuous_aggregate('{}', NULL, NULL)",
                    net.online_channels_hourly()
                );
                sqlx::query(&flush_nodes_sql)
                    .execute(pool)
                    .await
                    .expect("Failed to refresh continuous aggregate");
                sqlx::query(&flush_channels_sql)
                    .execute(pool)
                    .await
                    .expect("Failed to refresh continuous aggregate");
            }
            match net {
                fiber_dashbord_backend::Network::Mainnet => *mainnet_init = true,
                fiber_dashbord_backend::Network::Testnet => *testnet_init = true,
            }
        }
    }
}

static DAILY_COMMIT_TASK_HEARTBEAT: AtomicU64 = AtomicU64::new(0);
static HOURLY_FRESH_TASK_HEARTBEAT: AtomicU64 = AtomicU64::new(0);

async fn daily_commit() {
    let mut clock_timer = ClockTimer::new_daily(0, 11, true);
    let mut heartbeat_timer = tokio::time::interval(tokio::time::Duration::from_secs(60));
    heartbeat_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = heartbeat_timer.tick() => {
                let timestamp = Utc::now().timestamp() as u64;
                DAILY_COMMIT_TASK_HEARTBEAT.store(timestamp, Ordering::Release);
            }
            trigger_time = clock_timer.tick() => {
                let pool = get_pg_pool();
                daily_statistics(
                    pool,
                    Some(Utc::now() - chrono::Duration::days(20)),
                    NETS.iter(),
                )
                .await
                .unwrap();
                log::info!("Daily statistics committed at {}", trigger_time);
            }
        }
    }
}

async fn hourly_fresh() {
    let mut clock_timer = ClockTimer::new_interval_with_minute(5, 30, true);
    let mut heartbeat_timer = tokio::time::interval(tokio::time::Duration::from_secs(60));
    heartbeat_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = heartbeat_timer.tick() => {
                let timestamp = Utc::now().timestamp() as u64;
                HOURLY_FRESH_TASK_HEARTBEAT.store(timestamp, Ordering::Release);
            }
            trigger_time = clock_timer.tick() => {
                let pool = get_pg_pool();
                let nets = NETS.iter();
                for net in nets {
                    let refresh_nodes_sql = format!(
                        "REFRESH MATERIALIZED VIEW CONCURRENTLY {}",
                        net.mv_online_nodes()
                    );
                    let refresh_channels_sql = format!(
                        "REFRESH MATERIALIZED VIEW CONCURRENTLY {}",
                        net.mv_online_channels()
                    );
                    sqlx::query(&refresh_nodes_sql)
                        .execute(pool)
                        .await
                        .expect("Failed to refresh continuous aggregate");
                    sqlx::query(&refresh_channels_sql)
                        .execute(pool)
                        .await
                        .expect("Failed to refresh continuous aggregate");
                }
                log::info!("Hourly continuous aggregates refreshed at {}", trigger_time);
            }
        }
    }
}
