# VHS tape to render a short BranchBox teaser
# Requires: vhs (brew install charmbracelet/tap/vhs) and BranchBox available in PATH

Output demo-teaser.gif
Set FontSize 16
Set Width 1000
Set Height 650
Set Framerate 60
Set Theme "Dracula"

Type "# BranchBox: work on multiple features in parallel"
Enter
Sleep 400ms

Type "scripts/demo-teaser.sh --stack rust"
Enter

# Allow time for the script to seed, start features, list, and sync
Sleep 6s

Type "# Parallel features. Devcontainer sync in one command."
Enter
Sleep 800ms

Type "# Learn more: https://github.com/branchbox/branchbox"
Enter
Sleep 800ms
