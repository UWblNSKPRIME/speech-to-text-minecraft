# vocatalk mod

Fabric client mod for Minecraft 26.2. Receives transcripts from the `vocatalk` CLI over a
local TCP socket, shows the current draft on a HUD overlay and sends final transcripts to
the in-game chat. Part of the [speech-to-text-minecraft](../README.md) project; the CLI
side is documented in [`vocatalk/README.md`](../vocatalk/README.md).

## How it works

- The mod listens on `127.0.0.1:47832` (localhost only) for one client at a time and
  accepts reconnects.
- Every `draft` message updates a HUD line above the hotbar (white for `me`, green for
  `discord`). The draft disappears when the matching `final` arrives or after 10 s.
- Every `final` message is sent to chat as is; the CLI prefixes the speaker's chat name
  into `text` before sending (defaults `Вэвси` / `Аури`, see
  [`vocatalk/README.md`](../vocatalk/README.md)).

## Protocol

One JSON object per line (JSON-lines), UTF-8:

| Field    | Type   | Values                  | Meaning                              |
|----------|--------|-------------------------|--------------------------------------|
| `t`      | string | `"draft"` or `"final"`  | streaming update or finalized text   |
| `speaker`| string | `"me"` or `"discord"`   | audio channel the text came from     |
| `text`   | string | any                     | recognized text                      |

Example line:

```json
{"t":"final","speaker":"discord","text":"go to the nether"}
```

Unknown fields, unknown values and malformed lines are skipped (logged at debug level).

## Guards and limits

- Chat screen or inventory open, or not in a world → the text is **not** sent; it is
  copied to the clipboard instead and a short yellow notice appears on the HUD for 3 s.
- Text starting with `/` is treated as a command and goes to the clipboard the same way
  (the CLI sends such lines raw, without the speaker-name prefix).
- Final messages are truncated to 250 characters (chat limit).
- The socket binds to `127.0.0.1` only — nothing is exposed to the network.

## Building

Requires JDK 25 (`java -version` must print 25):

```
gradlew.bat build
```

The jar appears in `build/libs/vocatalk-<version>.jar`. The project uses Gradle wrapper
9.7.1 and Fabric Loom (`net.fabricmc.fabric-loom` 1.18.4); Minecraft 26.x is unobfuscated,
so no mappings are declared.

## Installing

Copy `vocatalk-<version>.jar` into the `mods` folder of a Fabric instance
(Fabric Loader >= 0.19.5 and Fabric API required), then launch the game.

## Testing without the vocatalk CLI

Join a world and run in PowerShell:

```powershell
$c = [Net.Sockets.TcpClient]::new('127.0.0.1', 47832)
$s = $c.GetStream()
$w = [IO.StreamWriter]::new($s); $w.NewLine = "`n"; $w.AutoFlush = $true
$w.WriteLine('{"t":"final","speaker":"discord","text":"hello from test"}')
$w.Dispose(); $c.Close()
```

You should see `hello from test` in chat (or the clipboard notice if the chat
window is open). Replace `"final"` with `"draft"` to see the HUD overlay line instead.
