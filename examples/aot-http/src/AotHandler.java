public final class AotHandler implements AotResponder {
    AotConfig config;
    String[] responseBodies;

    AotHandler(AotConfig config) {
        this.config = config;
        this.responseBodies = new String[] {config.body};
    }

    public int port() {
        return config.port();
    }

    public String body() {
        return "hello from " + responseBodies[0] + " fvm-aot http #" + port();
    }
}
