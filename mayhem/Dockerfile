FROM --platform=linux/amd64 rustlang/rust:nightly as builder

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update 
RUN apt-get install -y cmake clang
RUN cargo install cargo-fuzz

ADD . /Sunscreen/

WORKDIR /Sunscreen/fuzz/

ENV RUSTFLAGS="--cap-lints warn"
RUN cargo +nightly fuzz build

FROM --platform=linux/amd64 rustlang/rust:nightly

COPY --from=builder /Sunscreen/fuzz/target/x86_64-unknown-linux-gnu/release/multiply /

COPY --from=builder /Sunscreen/fuzz/target/x86_64-unknown-linux-gnu/release/cpu_scalar /

COPY --from=builder /Sunscreen/fuzz/target/x86_64-unknown-linux-gnu/release/cpu_ristretto_point /