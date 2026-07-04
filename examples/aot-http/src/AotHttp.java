import fvm.runtime.Http;

public final class AotHttp {
    public static void main(String[] args) {
        AotConfig config = new AotConfig(9000, "multi-class");
        AotResponder responder = new AotHandler(config);
        Http.respond(responder.port(), responder.body());
    }
}
