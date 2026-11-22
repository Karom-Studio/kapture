# Image du serveur de partage. Coolify construit ce Dockerfile.
# Le binaire est celui de l'application : le conteneur ne lance que `kapture serve`.

FROM rust:1-bookworm AS build

RUN apt-get update && apt-get install -y --no-install-recommends \
    clang \
    libclang-dev \
    libdbus-1-dev \
    libegl-dev \
    libgbm-dev \
    libgtk-3-dev \
    libpipewire-0.3-dev \
    libspa-0.2-dev \
    libvulkan-dev \
    libwayland-dev \
    libx11-dev \
    libxcb-randr0-dev \
    libxcb-shm0-dev \
    libxcb-xkb-dev \
    libxcb1-dev \
    libxdo-dev \
    libxext-dev \
    libxfixes-dev \
    libxi-dev \
    libxkbcommon-dev \
    libxkbcommon-x11-dev \
    libxrandr-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY vendor ./vendor
COPY src ./src
RUN cargo build --release --bin kapture

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libdbus-1-3 \
    libegl1 \
    libgbm1 \
    libgtk-3-0 \
    libvulkan1 \
    libwayland-client0 \
    libwayland-cursor0 \
    libwayland-egl1 \
    libx11-6 \
    libxcb-randr0 \
    libxcb-shm0 \
    libxcb-xkb1 \
    libxcb1 \
    libxdo3 \
    libxkbcommon-x11-0 \
    libxkbcommon0 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /src/target/release/kapture /usr/local/bin/kapture

ENV KAPTURE_BIND=0.0.0.0:8741
ENV KAPTURE_DIR=/data
VOLUME /data
EXPOSE 8741
ENTRYPOINT ["kapture", "serve"]
