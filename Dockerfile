# Tracecure — server + UI in one image.
#
#   docker build -t tracecure .
#   docker run -p 127.0.0.1:7789:7789 -v tracecure-data:/data tracecure
#
# The server has no authentication yet (PLAN.md, M6). Publish the port on
# loopback only and put an authenticating proxy in front of anything wider.

FROM rust:1.94-bookworm AS build

# Same trunk line CI uses (.github/workflows/ci.yml). Kept in its own layer:
# it is the slowest step and changes least.
RUN cargo install trunk --locked --version ^0.21

WORKDIR /src
COPY . .

# rust-toolchain.toml pins the channel and pulls the wasm32 target on first use.
RUN cargo build --release --locked -p tracecure-server

# ui/tracecure-ui is excluded from the cargo workspace, so it builds on its own.
# NO_COLOR is unset because trunk binds it to --no-color and rejects the value.
RUN cd ui/tracecure-ui && env -u NO_COLOR trunk build --release


FROM debian:bookworm-slim

RUN useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin tracecure \
    && mkdir /data && chown tracecure:tracecure /data

COPY --from=build /src/target/release/tracecure-server /usr/local/bin/tracecure-server
COPY --from=build /src/ui/tracecure-ui/dist /usr/share/tracecure/ui

USER tracecure
VOLUME /data
EXPOSE 7789

ENTRYPOINT ["tracecure-server"]
CMD ["serve", "--data", "/data", "--bind", "0.0.0.0:7789", "--ui-dist", "/usr/share/tracecure/ui"]
