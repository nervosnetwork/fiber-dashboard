FROM rust:1.95.0-slim AS build
WORKDIR /usr/src/fiber-dashbord-backend

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

COPY . .
RUN cd /usr/src/fiber-dashbord-backend && cargo build --release

FROM debian:13
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    libssl3 \
    libssl-dev \
    ca-certificates && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=build /usr/src/fiber-dashbord-backend/target/release/fiber-dashbord /app/fiber-dashbord
CMD ["/app/fiber-dashbord"]
