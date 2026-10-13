package net.vocatalk;

import com.google.gson.Gson;
import com.google.gson.JsonSyntaxException;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.net.InetSocketAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.function.Consumer;

final class TranscriptServer {
    private static final Logger LOGGER = LoggerFactory.getLogger("vocatalk");
    private static final Gson GSON = new Gson();
    private static final int PORT = 47832;

    private final Consumer<Transcript> handler;

    TranscriptServer(Consumer<Transcript> handler) {
        this.handler = handler;
    }

    void start() {
        Thread thread = new Thread(this::run, "vocatalk-socket");
        thread.setDaemon(true);
        thread.start();
    }

    private void run() {
        try (ServerSocket server = new ServerSocket()) {
            server.bind(new InetSocketAddress("127.0.0.1", PORT), 1);
            LOGGER.info("listening on 127.0.0.1:{}", PORT);
            while (true) {
                try (Socket socket = server.accept()) {
                    serve(socket);
                } catch (IOException e) {
                    LOGGER.warn("connection error: {}", e.getMessage());
                }
            }
        } catch (IOException e) {
            LOGGER.error("cannot listen on 127.0.0.1:{}: {}", PORT, e.getMessage());
        }
    }

    private void serve(Socket socket) throws IOException {
        LOGGER.info("client connected: {}", socket.getRemoteSocketAddress());
        BufferedReader in = new BufferedReader(new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8));
        String line;
        while ((line = in.readLine()) != null) {
            if (line.isBlank()) {
                continue;
            }
            Transcript transcript = parse(line);
            if (transcript != null) {
                handler.accept(transcript);
            }
        }
        LOGGER.info("client disconnected");
    }

    private static Transcript parse(String line) {
        Transcript transcript;
        try {
            transcript = GSON.fromJson(line, Transcript.class);
        } catch (JsonSyntaxException e) {
            LOGGER.debug("skipping malformed line: {}", line);
            return null;
        }
        if (transcript == null || transcript.text == null
                || (!"draft".equals(transcript.t) && !"final".equals(transcript.t))
                || (!"me".equals(transcript.speaker) && !"discord".equals(transcript.speaker))) {
            LOGGER.debug("skipping unexpected line: {}", line);
            return null;
        }
        return transcript;
    }
}
