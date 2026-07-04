# Perf HTTP Example

This app is a larger plain Java HTTP service used for repeatable FVM performance testing.

The benchmark harness generates extra reachable Java classes and a payload resource before compiling the JAR, then measures:

- Java compile and JAR packaging time
- FVM native-image/rootfs build time
- artifact sizes
- cold boot readiness time
- snapshot creation and restore
- host RSS and guest RSS
- projected host density with `fvm math`

Run on the Linux/KVM server:

```bash
scripts/perf-native
```

Useful overrides:

```bash
ITERATIONS=20 HOST_PORT=18081 ROOTFS_SIZE=192M scripts/perf-native
```
