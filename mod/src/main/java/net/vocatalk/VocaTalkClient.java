package net.vocatalk;

import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.rendering.v1.hud.HudElementRegistry;
import net.minecraft.resources.Identifier;

public class VocaTalkClient implements ClientModInitializer {
    public static final String MOD_ID = "vocatalk";

    @Override
    public void onInitializeClient() {
        DraftStore drafts = new DraftStore();
        HudElementRegistry.addLast(Identifier.fromNamespaceAndPath(MOD_ID, "overlay"), drafts::render);
        new TranscriptServer(drafts::onMessage).start();
    }
}
