# Plain HTTP Example

This is the MVP Java shape: one plain Java HTTP service with no framework.

Build a JAR with a Java 25-compatible JDK:

```bash
javac --release 25 -d out src/App.java
jar --create --file app.jar --main-class App -C out .
```

Build an FVM artifact:

```bash
../../target/debug/fvm build app.jar --mode native --java 25 --kernel /path/to/vmlinux --port 8080:8080
```

Run it on a Linux/KVM host:

```bash
../../target/debug/fvm run app.fvm --port 8080:8080
curl http://127.0.0.1:8080/health
```
