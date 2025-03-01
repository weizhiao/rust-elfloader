FROM ubuntu:24.04 

# Install qemu
RUN sed -i 's/archive.ubuntu.com/mirrors.aliyun.com/g' /etc/apt/sources.list && \
    sed -i 's/security.ubuntu.com/mirrors.aliyun.com/g' /etc/apt/sources.list && \
    apt-get update && \
    apt-get install -y \
    wget \
    tar \
    curl \
    qemu-user-static  \
    gdb-multiarch

# Install loongarch64 musl gcc
RUN mkdir -p /opt/loongarch64

WORKDIR /opt/loongarch64
RUN wget https://github.com/LoongsonLab/oscomp-toolchains-for-oskernel/releases/download/loongarch64-linux-musl-cross-gcc-13.2.0/loongarch64-linux-musl-cross.tgz && \
    tar -xzf loongarch64-linux-musl-cross.tgz --strip-components=1 && \
    rm loongarch64-linux-musl-cross.tgz  

ENV PATH="/opt/loongarch64/bin:${PATH}"

# Install Rust
ENV RUSTUP_DIST_SERVER=https://mirrors.ustc.edu.cn/rust-static \
    RUSTUP_UPDATE_ROOT=https://mirrors.ustc.edu.cn/rust-static/rustup \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal --default-toolchain stable && \
    echo "Rustup installation completed" 

# Install target
RUN rustup target add loongarch64-unknown-linux-musl &&\
    rustup target add riscv64gc-unknown-linux-gnu &&\
    rustup target add aarch64-unknown-linux-gnu