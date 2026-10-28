# GTK, ALSA and the wxDragon toolchain, then the release build and the AppImage.
# libssl-dev: wxdragon-sys fetches wxWidgets over native-tls while it builds.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
	&& apt-get install -y --no-install-recommends \
		build-essential cmake ninja-build clang libclang-dev \
		libgtk-3-dev libasound2-dev libssl-dev pkg-config \
		curl ca-certificates file xvfb fuse libfuse2 \
	&& rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
ENV PATH=/root/.cargo/bin:$PATH
ENV LIBCLANG_PATH=/usr/lib/llvm-18/lib

WORKDIR /work
CMD ["cargo", "xtask", "dist-linux"]
