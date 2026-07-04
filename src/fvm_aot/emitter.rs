use super::{AotProgram, HttpServer};

pub(super) fn emit_c(program: &AotProgram) -> String {
    if let Some(server) = &program.http_server {
        return emit_http_server_c(server);
    }

    let mut c = String::from("#include <stdio.h>\n#include <stddef.h>\n\nint main(void) {\n");
    for (index, bytes) in program.printlns.iter().enumerate() {
        c.push_str("  static const unsigned char msg");
        c.push_str(&index.to_string());
        c.push_str("[] = {");
        for byte in bytes.iter().copied().chain([b'\n']) {
            c.push_str(&format!("0x{byte:02x},"));
        }
        c.push_str("};\n  fwrite(msg");
        c.push_str(&index.to_string());
        c.push_str(", 1, sizeof(msg");
        c.push_str(&index.to_string());
        c.push_str("), stdout);\n");
    }
    c.push_str("  return 0;\n}\n");
    c
}

fn emit_http_server_c(server: &HttpServer) -> String {
    let mut c = String::from(
        "#include <arpa/inet.h>\n#include <netinet/in.h>\n#include <stdio.h>\n#include <string.h>\n#include <sys/socket.h>\n#include <unistd.h>\n\nint main(void) {\n",
    );
    c.push_str("  static const unsigned char body[] = {");
    for byte in &server.body {
        c.push_str(&format!("0x{byte:02x},"));
    }
    c.push_str("};\n");
    c.push_str(&format!(
        "  static const char header[] = \"HTTP/1.1 200 OK\\r\\nContent-Length: {}\\r\\nConnection: close\\r\\n\\r\\n\";\n",
        server.body.len()
    ));
    c.push_str(
        "  int server_fd = socket(AF_INET, SOCK_STREAM, 0);\n  if (server_fd < 0) return 1;\n  int one = 1;\n  setsockopt(server_fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));\n  struct sockaddr_in addr;\n  memset(&addr, 0, sizeof(addr));\n  addr.sin_family = AF_INET;\n  addr.sin_addr.s_addr = htonl(INADDR_ANY);\n",
    );
    c.push_str(&format!("  addr.sin_port = htons({});\n", server.port));
    c.push_str(
        "  if (bind(server_fd, (struct sockaddr *)&addr, sizeof(addr)) != 0) return 2;\n  if (listen(server_fd, 128) != 0) return 3;\n  for (;;) {\n    int client = accept(server_fd, NULL, NULL);\n    if (client < 0) continue;\n    char request[1024];\n    ssize_t read_result = read(client, request, sizeof(request));\n    if (read_result < 0) { close(client); continue; }\n    ssize_t header_result = write(client, header, sizeof(header) - 1);\n    if (header_result < 0) { close(client); continue; }\n    ssize_t body_result = write(client, body, sizeof(body));\n    if (body_result < 0) { close(client); continue; }\n    close(client);\n  }\n}\n",
    );
    c
}
