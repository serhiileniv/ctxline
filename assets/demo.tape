# Regenerates assets/demo.gif:  vhs assets/demo.tape
#
# The frames come from scripts/gif.sh, which drives the real binary -- nothing
# here is hand-drawn, so a palette or format change lands in the GIF too.
#
# Background is near-black on purpose: the palette is measured against pure
# black (see the Colors section of the README), and a lighter theme would show
# contrast the README does not claim.

Output assets/demo.gif

Set Shell        "bash"
Set FontFamily   "Menlo"
Set FontSize     22
Set Width        920
Set Height       212
Set Padding      24
Set TypingSpeed  0ms
Set Framerate    24
Set PlaybackSpeed 1
Set Theme { "name": "ctxline", "background": "#08080A", "foreground": "#C6C6C6", "cursor": "#C6C6C6", "selection": "#2A2A30", "black": "#08080A", "red": "#D75F5F", "green": "#5FAF5F", "yellow": "#D7AF5F", "blue": "#5F87D7", "magenta": "#D75F87", "cyan": "#5FAFAF", "white": "#C6C6C6", "brightBlack": "#808080", "brightRed": "#FF8787", "brightGreen": "#87D787", "brightYellow": "#FFD787", "brightBlue": "#87AFD7", "brightMagenta": "#FF87AF", "brightCyan": "#87D7D7", "brightWhite": "#FFFFFF" }

Hide
Type "clear && ./scripts/gif.sh"
Enter
Show
Sleep 6s
