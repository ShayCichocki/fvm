ARG GRAALVM_IMAGE=ghcr.io/graalvm/native-image-community:25.0.2
FROM ${GRAALVM_IMAGE}

USER root

RUN microdnf install -y \
      cargo \
      rust \
      e2fsprogs \
      iproute \
      iptables \
      procps-ng \
      util-linux \
      findutils \
      tar \
      gzip \
      curl \
      make \
    && microdnf clean all

WORKDIR /src/fvm
COPY . /src/fvm
RUN cargo build --release

ENV PATH="/src/fvm/target/release:${PATH}"

ENTRYPOINT ["fvm"]
