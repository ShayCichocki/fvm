import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import perf.gen.GeneratedRegistry;

public final class PerfApp {
    private static final byte[] PAYLOAD = readResource("payload.txt");
    private static final String[] LABELS = GeneratedRegistry.labels();
    private static final Map<String, Integer> INDEX = buildIndex();
    private static final int BOOT_CHECKSUM = computeChecksum(PAYLOAD) ^ GeneratedRegistry.checksum();
    private static final long STARTED_AT = System.currentTimeMillis();

    public static void main(String[] args) throws Exception {
        warmup();

        int port = Integer.parseInt(System.getenv().getOrDefault("PORT", "8080"));
        HttpServer server = HttpServer.create(new InetSocketAddress("0.0.0.0", port), 128);
        server.createContext("/health", exchange -> respond(exchange, 200, "ok " + BOOT_CHECKSUM + "\n"));
        server.createContext("/compute", PerfApp::compute);
        server.createContext("/json", PerfApp::json);
        server.createContext("/search", PerfApp::search);
        server.createContext("/alloc", PerfApp::alloc);
        server.start();

        System.out.println("perf app listening on " + port + " checksum=" + BOOT_CHECKSUM);
    }

    private static void compute(HttpExchange exchange) throws IOException {
        int rounds = intParam(exchange.getRequestURI(), "rounds", 2500);
        long value = BOOT_CHECKSUM;
        for (int i = 0; i < rounds; i++) {
            value = Long.rotateLeft(value * 1_103_515_245L + i + PAYLOAD[i % PAYLOAD.length], 11);
            value ^= GeneratedRegistry.mix((int) value);
        }
        respond(exchange, 200, Long.toUnsignedString(value) + "\n");
    }

    private static void json(HttpExchange exchange) throws IOException {
        String body = "{"
            + "\"startedAt\":\"" + Instant.ofEpochMilli(STARTED_AT) + "\","
            + "\"payloadBytes\":" + PAYLOAD.length + ","
            + "\"labels\":" + LABELS.length + ","
            + "\"indexEntries\":" + INDEX.size() + ","
            + "\"checksum\":" + BOOT_CHECKSUM
            + "}\n";
        respond(exchange, 200, body, "application/json");
    }

    private static void search(HttpExchange exchange) throws IOException {
        String q = stringParam(exchange.getRequestURI(), "q", "unit").toLowerCase();
        List<String> matches = new ArrayList<>();
        for (String label : LABELS) {
            if (label.toLowerCase().contains(q)) {
                matches.add(label);
            }
            if (matches.size() == 20) {
                break;
            }
        }
        respond(exchange, 200, String.join("\n", matches) + "\n");
    }

    private static void alloc(HttpExchange exchange) throws IOException {
        int kib = intParam(exchange.getRequestURI(), "kib", 512);
        byte[][] chunks = new byte[Math.max(1, kib / 64)][64 * 1024];
        int checksum = 0;
        for (int i = 0; i < chunks.length; i++) {
            chunks[i][0] = (byte) i;
            chunks[i][chunks[i].length - 1] = (byte) (i * 31);
            checksum += chunks[i][0] + chunks[i][chunks[i].length - 1];
        }
        respond(exchange, 200, "allocated_kib=" + (chunks.length * 64) + " checksum=" + checksum + "\n");
    }

    private static void warmup() {
        long value = BOOT_CHECKSUM;
        for (int i = 0; i < 10_000; i++) {
            value = Long.rotateLeft(value ^ GeneratedRegistry.mix(i), 7);
        }
        if (value == 42) {
            System.out.println("impossible warmup guard");
        }
    }

    private static Map<String, Integer> buildIndex() {
        Map<String, Integer> index = new HashMap<>();
        for (String label : LABELS) {
            index.put(label, label.hashCode());
        }
        String payload = new String(PAYLOAD, StandardCharsets.UTF_8);
        for (String line : payload.split("\\R")) {
            if (!line.isEmpty()) {
                index.put(line, computeChecksum(line.getBytes(StandardCharsets.UTF_8)));
            }
        }
        return index;
    }

    private static byte[] readResource(String name) {
        try (InputStream input = PerfApp.class.getClassLoader().getResourceAsStream(name)) {
            if (input == null) {
                return ("missing-resource:" + name).getBytes(StandardCharsets.UTF_8);
            }
            ByteArrayOutputStream output = new ByteArrayOutputStream();
            byte[] buffer = new byte[16 * 1024];
            int read;
            while ((read = input.read(buffer)) >= 0) {
                output.write(buffer, 0, read);
            }
            return output.toByteArray();
        } catch (IOException error) {
            throw new IllegalStateException("failed to read resource " + name, error);
        }
    }

    private static int computeChecksum(byte[] bytes) {
        int value = 0x4f1bbcdd;
        for (byte b : bytes) {
            value = Integer.rotateLeft(value ^ (b & 0xff), 5) * 16_777_619;
        }
        return value;
    }

    private static int intParam(URI uri, String key, int fallback) {
        String value = stringParam(uri, key, null);
        if (value == null) {
            return fallback;
        }
        try {
            return Integer.parseInt(value);
        } catch (NumberFormatException ignored) {
            return fallback;
        }
    }

    private static String stringParam(URI uri, String key, String fallback) {
        String query = uri.getRawQuery();
        if (query == null || query.isEmpty()) {
            return fallback;
        }
        for (String part : query.split("&")) {
            int index = part.indexOf('=');
            if (index > 0 && part.substring(0, index).equals(key)) {
                return part.substring(index + 1);
            }
        }
        return fallback;
    }

    private static void respond(HttpExchange exchange, int status, String body) throws IOException {
        respond(exchange, status, body, "text/plain; charset=utf-8");
    }

    private static void respond(HttpExchange exchange, int status, String body, String contentType) throws IOException {
        byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().set("Content-Type", contentType);
        exchange.sendResponseHeaders(status, bytes.length);
        try (OutputStream output = exchange.getResponseBody()) {
            output.write(bytes);
        }
    }
}
