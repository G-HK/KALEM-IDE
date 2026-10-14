# Linux check for the desktop app: builds kalem-ui and starts it on a virtual screen with software
# Vulkan (lavapipe), so it runs anywhere Docker does, with no GPU. Used locally and by CI:
#
#   docker build -t kalem-linux-check deployments/linux
#   docker run --rm -v "$PWD:/src" -v kalem-target:/src/target -v kalem-cargo:/usr/local/cargo/registry kalem-linux-check
FROM ubuntu:latest
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git build-essential pkg-config \
      libx11-dev libx11-xcb-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
      libfontconfig1-dev libfreetype-dev libvulkan-dev libssl-dev \
      xvfb mesa-vulkan-drivers imagemagick x11-utils \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --component clippy,rustfmt
WORKDIR /src
CMD ["bash", "deployments/linux/check.sh"]
