# Build stage
FROM rust:alpine AS builder

WORKDIR /app

# Install build dependencies
# build-base: for compiling C dependencies (e.g. ring)
RUN apk add --no-cache build-base perl

COPY . .

# Build the release binary
# Since we are on Alpine, this produces a musl-linked static binary automatically
RUN cargo build --release

# Runtime stage
FROM alpine:latest

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/apex /usr/local/bin/apex

# Expose the default port
EXPOSE 12356

# Set entrypoint
ENTRYPOINT ["apex"]
