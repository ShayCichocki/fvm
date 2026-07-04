public final class AotConfig {
    int base;
    int[] offsets;
    String body;

    AotConfig(int base, String body) {
        this.base = base;
        this.offsets = new int[] {40, 50};
        this.body = body;
    }

    int port() {
        return base + offsets[0] + offsets[1] + offsets.length - 2;
    }
}
