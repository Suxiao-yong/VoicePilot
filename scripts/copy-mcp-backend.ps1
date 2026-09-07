# Re-run after `cargo clean` or fresh clone: puts the UIA backend exe next
# to the app binaries so `resolve_server_command` finds it with zero config.
# The exe itself is NOT committed (see .gitignore `tools/mcp-windows/`).
# Usage (from repo root): powershell -File scripts/copy-mcp-backend.ps1
$src = Join-Path (Join-Path (Join-Path (Join-Path (Join-Path $PSScriptRoot "..") "tools") "mcp-windows") "server") "Sbroenne.WindowsMcp.exe"
if (-not (Test-Path $src)) {
    Write-Error "missing $src — download windows-mcp-server-win-x64.zip from https://github.com/sbroenne/mcp-windows/releases and extract it to tools/mcp-windows/server/"
    exit 1
}
foreach ($dir in @("voicepilot/target/release", "voicepilot/target/debug")) {
    $dest = Join-Path (Join-Path $PSScriptRoot "..") $dir
    if (Test-Path $dest) {
        Copy-Item $src (Join-Path $dest "Sbroenne.WindowsMcp.exe") -Force
        Write-Host "copied -> $dest"
    } else {
        Write-Host "skip (no such dir): $dest"
    }
}
