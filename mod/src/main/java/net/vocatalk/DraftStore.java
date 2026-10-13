package net.vocatalk;

import net.minecraft.client.DeltaTracker;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.client.gui.screens.ChatScreen;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.client.gui.screens.inventory.InventoryScreen;
import net.minecraft.client.multiplayer.ClientPacketListener;

import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

final class DraftStore {
    private static final long DRAFT_TIMEOUT_MS = 10_000;
    private static final long NOTICE_MS = 3_000;
    private static final int MAX_CHAT_LENGTH = 250;
    private static final int COLOR_ME = 0xFFFFFFFF;
    private static final int COLOR_DISCORD = 0xFF55FF55;
    private static final int COLOR_NOTICE = 0xFFFFFF55;
    private static final List<String> SPEAKER_ORDER = List.of("me", "discord");

    private final Map<String, Draft> drafts = new ConcurrentHashMap<>();
    private volatile Notice notice;

    private record Draft(String text, long updatedAt) {
    }

    private record Notice(String text, long until) {
    }

    void onMessage(Transcript transcript) {
        if ("final".equals(transcript.t)) {
            drafts.remove(transcript.speaker);
            Minecraft.getInstance().execute(() -> sendFinal(transcript.speaker, transcript.text));
        } else {
            drafts.put(transcript.speaker, new Draft(transcript.text, System.currentTimeMillis()));
        }
    }

    void render(GuiGraphicsExtractor graphics, DeltaTracker tracker) {
        Minecraft client = Minecraft.getInstance();
        long now = System.currentTimeMillis();

        drafts.values().removeIf(draft -> now - draft.updatedAt() > DRAFT_TIMEOUT_MS);
        Notice current = notice;
        if (current != null && now > current.until()) {
            notice = null;
            current = null;
        }

        int y = graphics.guiHeight() - 70;
        if (current != null) {
            graphics.text(client.font, current.text(), 4, y, COLOR_NOTICE);
            y += client.font.lineHeight + 1;
        }
        for (String speaker : SPEAKER_ORDER) {
            Draft draft = drafts.get(speaker);
            if (draft != null) {
                graphics.text(client.font, draft.text(), 4, y, colorOf(speaker), true);
                y += client.font.lineHeight + 1;
            }
        }
    }

    private void sendFinal(String speaker, String rawText) {
        Minecraft client = Minecraft.getInstance();
        String text = "discord".equals(speaker) ? "Discord: " + rawText : rawText;
        if (text.length() > MAX_CHAT_LENGTH) {
            text = text.substring(0, MAX_CHAT_LENGTH);
        }

        Screen screen = client.gui.screen();
        ClientPacketListener connection = client.getConnection();
        if (connection == null) {
            copyToClipboard(client, text, "vocatalk: not in world, copied to clipboard");
        } else if (screen instanceof ChatScreen || screen instanceof InventoryScreen) {
            copyToClipboard(client, text, "vocatalk: chat busy, copied to clipboard");
        } else {
            connection.sendChat(text);
        }
    }

    private void copyToClipboard(Minecraft client, String text, String noticeText) {
        client.keyboardHandler.setClipboard(text);
        notice = new Notice(noticeText, System.currentTimeMillis() + NOTICE_MS);
    }

    private static int colorOf(String speaker) {
        return "me".equals(speaker) ? COLOR_ME : COLOR_DISCORD;
    }
}
