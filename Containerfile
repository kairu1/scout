# Operation SCOUT — container image for deployed agents
#
# Ubuntu base with Rust toolchain, tmux for multi-pane multiplexing,
# SQLite headers (the index will always be SQLite), and Claude Code.
# Built from the doctrine established in agentic_vm_guide.
#
# Build:   podman build -t scout:latest .
# Run:     see ops/phase-0-mobilize.md

FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
ENV TZ=UTC

# System packages
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates \
      curl \
      git \
      tmux \
      htop \
      jq \
      less \
      vim \
      bash-completion \
      build-essential \
      pkg-config \
      libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

# Non-root user mapped to host UID 1000 (standard macOS first user);
# the mount in podman uses :Z relabeling so this user can read/write.
ARG USER=scout
ARG UID=1000
RUN useradd -m -u ${UID} -s /bin/bash ${USER}

USER ${USER}
WORKDIR /home/${USER}

# Rust toolchain (stable, minimal profile + the tools officers actually use)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain stable --profile minimal
ENV PATH="/home/${USER}/.cargo/bin:${PATH}"
RUN rustup component add rustfmt clippy

# Claude Code — installed per project doctrine (guide parts 1-4)
RUN curl -fsSL https://claude.ai/install.sh | bash
ENV PATH="/home/${USER}/.local/bin:${PATH}"

# Workspace — mounted from host at runtime (~/@kairu/@projects/@shell/scout)
WORKDIR /workspace

CMD ["/bin/bash"]
