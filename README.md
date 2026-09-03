## Fiber Dashbord

## Run with Docker Compose

Copy the example environment file first:

```bash
cp .env.example .env
```

For normal or production usage, pull the published backend image and start the stack:

```bash
docker compose pull
docker compose up -d
```

You can also let Compose pull during startup:

```bash
docker compose up -d --pull always
```

To deploy an immutable branch build instead of `latest`, override `FIBER_DASHBOARD_IMAGE` with a tag like `<normalized-branch>-<7charsha>`:

```bash
FIBER_DASHBOARD_IMAGE=ghcr.io/nervosnetwork/fiber-dashboard-backend:feature-api-cache-abcdef1 docker compose up -d
```

For local backend image builds, use the override file:

```bash
docker compose -f compose.yaml -f compose.build.yaml up -d --build
```

Running containers will not restart automatically just because the remote `latest` tag changed. Run a Compose update command again when you want to pull and recreate with a newer image.

http api default on 8080, method list:

```
/nodes_hourly?page=0&sort_by=region/last_seen/channel_count&order=asc/desc
/channels_hourly?page=0
/nodes_nearly_monthly?page=0&start=%Y-%m-%d&end=%Y-%m-%d start/end is optional
/channels_nearly_monthly?page=0&start=%Y-%m-%d&end=%Y-%m-%d start/end is optional
/node_udt_infos?node_id=0x...
/analysis_hourly?end=2012-12-12 12:12:12+0000
/channel_state?channel_outpoint=0x..
/group_channel_by_state?state=open/closed_cooperative/closed_waiting_onchain_settlement/closed_uncooperative&page=0&sort_by=create_time/last_commit_time&order=asc/desc&fuzz_name=Cr&asset_name=RUSD
/channel_count_by_state
/channel_count_by_asset
/channel_info?channel_outpoint=0x..
/node_info?node_id=0x..
/channels_by_node_id?node_id=0x..&page=0&sort_by=create_time/last_commit_time/asset&order=asc/desc
/nodes_by_region?region=HK&page=0&sort_by=region/last_seen/channel_count&order=asc/desc
/nodes_fuzzy_by_name?node_name=Cr&page=0&sort_by=region/last_seen/channel_count&order=asc/desc
/channel_capacity_distribution
/all_region
/health_check
post /nodes_by_udt body={ udt: Script }
post /analysis need json body
```

All apis that include paging functions have a page_size parameter. The default is 500, and the maximum is 500. It can be adjusted by passing parameters.

/analysis body:
| Parameter | Type                          | Description                                                    |
| --------- | ----------------------------- | -------------------------------------------------------------- |
| start     | string(%Y-%m-%d)              | Start date (e.g., `2025-08-01`) (optional, defaults to today) |
| end       | string(%Y-%m-%d)              | End date (optional)                                            |
| range     | enum                          | Time span, frontend passes `1M/3M/6M/1Y/2Y` (optional, auto-calculates start/end if provided) |
| interval  | enum                          | Aggregation granularity: `day` (default)                      |
| fields    | string\[]                     | Required metrics, e.g., `["channels","capacity","nodes", "asset"]` (defaults to all if not provided), capacity point is [sum, avg, min, max, median] |


All APIs have a parameter called `net`, which can be testnet or mainnet. The default is mainnet.
