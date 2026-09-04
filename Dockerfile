# Multi-stage lightweight build for InterMCP
FROM rust:alpine AS builder

WORKDIR /usr/src/intermcp
RUN apk add --no-cache musl-dev

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

# Production minimal runtime
FROM alpine:3.20

RUN apk add --no-cache ca-certificates tzdata

COPY --from=builder /usr/src/intermcp/target/release/intermcp /usr/local/bin/intermcp

# Expose default HTTP/SSE port
EXPOSE 8080

ENTRYPOINT ["intermcp"]
CMD ["serve"]
