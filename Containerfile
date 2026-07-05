# scout — product image (two-stage). Build the pinned toolchain against
# the repo, ship a slim runtime with just the binary. The Phase 0 agent
# battlefield image this file used to hold moved to the ops rig; this
# is the tool itself.
#
# Build:  podman build -t scout .
# Run:    podman run --rm -it -v "$PWD:/work" scout index /work
#         podman run --rm -it scout --help

FROM rust:1.96 AS build
WORKDIR /src
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN useradd -m -s /bin/bash scout
COPY --from=build /src/target/release/scout /usr/local/bin/scout
COPY shell/scout.bash /usr/local/share/scout/scout.bash
COPY examples/config.toml /usr/local/share/scout/config.toml.example
USER scout
WORKDIR /home/scout
ENTRYPOINT ["scout"]
