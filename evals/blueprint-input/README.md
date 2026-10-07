# Blueprint input to text

This DEV-only check proves that a voice file is materialized, converted by the
available converter capability, and delivered to the creating agent tab.

Prerequisites:

- Build and run a named DEV instance called `bpdev`.
- Install the Transcriber capability plugin and complete its first-use setup.
- Run the commands from the owner agent tab in that DEV instance.
- Install the fixture generator's `System.Speech` runtime when regenerating the WAV.

Create a blueprint and copy its id from the JSON output:

```bash
CODETERM_INSTANCE=bpdev bash evals/blueprint-input/live.sh create
```

Wait for the owner tab to receive the input result notice, then verify its
durable AgentPad body and the run cleanup:

```bash
CODETERM_INSTANCE=bpdev bash evals/blueprint-input/live.sh verify <blueprint_id>
```

The verification exits zero only when the convert step is done with a summary
such as `audio quick-brown-fox.wav -> text, 43 chars`, the blueprint result is
delivered, an inbox body headed `Input result (untrusted data)` contains “quick
brown fox”, and the run directory is gone. It prints the observed transcript
body. The WAV is at most 400 KB and speaks “The quick brown fox jumps over the
lazy dog.”

Regenerate the WAV on Windows with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File evals/blueprint-input/make-fixture.ps1
```

On macOS, the equivalent source audio can be produced with `say -o
evals/blueprint-input/quick-brown-fox.aiff "The quick brown fox jumps over the
lazy dog."` and converted to mono 16 kHz PCM WAV with `afconvert`.
