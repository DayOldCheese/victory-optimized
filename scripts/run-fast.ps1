# Runs a title with the performance profile: upstream's measured switch
# set (the list is in crates/victory-core/src/profiles.rs, one place for
# this script, the command line and the GUI).
#
# Usage (from the repository folder, in PowerShell):
#   .\scripts\run-fast.ps1 "D:\path\to\Game.exe"
#   .\scripts\run-fast.ps1 "D:\path\to\Game.exe" -Log D:\frames.txt
#   .\scripts\run-fast.ps1 "D:\path\to\Game.exe" -Experimental   # + indirect windows/gathers (tied to lost cards upstream)

param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$Log = "",
    [switch]$Experimental,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$TitleArgs
)

$victory = Join-Path $PSScriptRoot "..\target\release\victory.exe"
if (-not (Test-Path $victory)) {
    Write-Error "Build first: cargo build --release -p victory --features diagnostics"
    exit 1
}
$profile = if ($Experimental) { "experimental" } else { "performance" }
if ($Log -ne "") { $env:VICTORY_FRAME_LOG_FILE = $Log }
try {
    if ($TitleArgs) {
        & $victory run $Exe --execute --profile $profile -- @TitleArgs
    } else {
        & $victory run $Exe --execute --profile $profile
    }
} finally {
    if ($Log -ne "") { Remove-Item Env:VICTORY_FRAME_LOG_FILE -ErrorAction SilentlyContinue }
}
