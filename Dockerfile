FROM rust:alpine AS build

COPY Cargo.toml Cargo.lock ./

# Build with a dummy main to pre-build dependencies
RUN apk add --no-cache alpine-sdk &&  \
 mkdir src && \
 echo "fn main(){}" > src/main.rs && \
 cargo build --release --target x86_64-unknown-linux-musl && \
 rm -r src

COPY src/* ./src/

RUN touch src/main.rs && \
  cargo build --release --target x86_64-unknown-linux-musl

FROM scratch

COPY --from=build /target/x86_64-unknown-linux-musl/release/taspromto /
EXPOSE 80

CMD ["/taspromto"]