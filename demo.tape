# demo.tape — generates assets/demo.gif for the README.
#
# Requires: vhs (github.com/charmbracelet/vhs), plus ttyd and ffmpeg.
# Run a release build first so the build step below is a fast no-op, then:
#
#     cargo build --release
#     vhs demo.tape
#
# Output is written to assets/demo.gif.

Output assets/demo.gif

Set Shell bash
Set FontSize 18
Set Width 1000
Set Height 640
Set Padding 20
Set Theme "Dracula"

# Build off-camera and clear the screen so the GIF opens on a clean prompt.
Hide
Type "cargo build --release" Enter
Sleep 3s
Type "clear" Enter
Sleep 500ms
Show

# Smooth sine sweep — best for watching the needle move.
Type "./feed.sh sine | ./target/release/termtaco --title RATE --border-label termtaco" Enter
Sleep 10s

# Quit cleanly (termtaco reads keys from the controlling tty).
Type "q"
Sleep 1s
