FROM aegooby/rust-fuzz:latest as builder

ADD . /repo
WORKDIR /repo

## TODO: ADD YOUR BUILD INSTRUCTIONS HERE.
RUN cd fuzz && ${HOME}/.cargo/bin/cargo fuzz build

# Package Stage
FROM ubuntu:20.04

## TODO: Change <Path in Builder Stage>
COPY --from=builder /repo/fuzz/target/x86_64-unknown-linux-gnu/release/fuzz_decryption /
COPY --from=builder /repo/fuzz/target/x86_64-unknown-linux-gnu/release/fuzz_signature_validation /