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
      sqlite3 \
    && rm -rf /var/lib/apt/lists/*

# Non-root user. Ubuntu 24.04 ships a default `ubuntu` user at UID 1000,
# so we remove it first (idempotent — `|| true` tolerates future base images
# that drop the default) before creating our own officer account.
ARG USER=scout
ARG UID=1000
RUN (userdel -r ubuntu 2>/dev/null || true) \
 && useradd -m -u ${UID} -s /bin/bash ${USER}

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
