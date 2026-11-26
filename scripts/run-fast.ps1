# Runs a title with the switch set upstream measured its best frame rates
# with (fxceless/victory-fork, diagnostics/README.md, runs 278-316). The
# defaults are much slower: upstream notes frames of 3 to 6 seconds with
# them. These switches are experimental, which is why they are off by
# default. Upstream's runs with them lost the card after 30 to 50 minutes.
#
# Usage (from the repository folder, in PowerShell):
#   .\scripts\run-fast.ps1 "D:\path\to\Game.exe"
#   .\scripts\run-fast.ps1 "D:\path\to\Game.exe" -Log D:\frames.txt
#   .\scripts\run-fast.ps1 "D:\path\to\Game.exe" -Safe   # without indirect windows/gathers

param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$Log = "",
    [switch]$Safe,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$TitleArgs
)

$switches = @{
    VICTORY_LEVELS_ON_DEMAND = "1"
    VICTORY_ASYNC_SUBMIT     = "1"
    VICTORY_QUEUE2           = "1"
    VICTORY_TEXTURE_NO_WAIT  = "1"
    VICTORY_MERGE_GAP        = "64"
    VICTORY_MERGE_RUNS       = "4096"
    VICTORY_STAGING_POOL     = "0"
    VICTORY_COPY_PLAIN       = "1"
    VICTORY_JOIN_PARTS       = "1"
    VICTORY_BACK_OWN_VIEW    = "1"
    VICTORY_CARD_REHASH_RUNS = "1"
    # Upstream run 295/296: 2048 left textures black when the card ran out
    # of room; 1024 is the middle ground for a 10-12 GB card.
    VICTORY_CARD_WINDOWS_MB  = "1024"
    VICTORY_CARD_BUFFERS_MB  = "1280"
}
if (-not $Safe) {
    # The largest single GPU saving (one clamp pass per window of draws, not
    # per draw), and the switches upstream suspects in its lost-card runs.
    $switches.VICTORY_INDIRECT_WINDOW = "1"
    $switches.VICTORY_INDIRECT_GATHER = "1"
}
if ($Log -ne "") {
    $switches.VICTORY_FRAME_LOG_FILE = $Log
}

foreach ($k in $switches.Keys) {
    Set-Item -Path "Env:$k" -Value $switches[$k]
}
Write-Host "Switches set:" ($switches.Keys | Sort-Object | ForEach-Object { "$_=$($switches[$_])" }) -Separator "`n  "

$victory = Join-Path $PSScriptRoot "..\target\release\victory.exe"
if (-not (Test-Path $victory)) {
    Write-Error "Build first: cargo build --release -p victory --features diagnostics"
    exit 1
}
if ($TitleArgs) {
    & $victory run $Exe --execute -- @TitleArgs
} else {
    & $victory run $Exe --execute
}

# The switches live in this PowerShell session only; remove them so a
# later plain run uses the defaults again.
foreach ($k in $switches.Keys) {
    Remove-Item -Path "Env:$k" -ErrorAction SilentlyContinue
}
