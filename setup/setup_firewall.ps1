# --- setup_firewall.ps1 (Fixed Version) ---

$TargetExeName = "droptea_core.exe"
$RuleName = "DropTea Allow All"

Write-Host "--- DropTea Firewall Setup ---" -ForegroundColor Cyan

# Get the directory where this script is located
$ScriptPath = Split-Path -Parent $MyInvocation.MyCommand.Definition

# Define paths explicitly (No complex arrays)
$Path1 = Join-Path -Path $ScriptPath -ChildPath $TargetExeName
$Path2 = Join-Path -Path $ScriptPath -ChildPath "target\debug\$TargetExeName"
$Path3 = Join-Path -Path $ScriptPath -ChildPath "..\target\debug\$TargetExeName"

$ExePath = $null

# Check Path 1: Same folder
if (Test-Path $Path1) {
    $ExePath = $Path1
}
# Check Path 2: Inside target/debug
elseif (Test-Path $Path2) {
    $ExePath = $Path2
}
# Check Path 3: Parent target/debug (for setup folder)
elseif (Test-Path $Path3) {
    $ExePath = $Path3
}

# Resolve full path to remove ".."
if ($ExePath) {
    $ExePath = [System.IO.Path]::GetFullPath($ExePath)
}

# Final Check
if (-Not $ExePath) {
    Write-Host "ERROR: File '$TargetExeName' not found!" -ForegroundColor Red
    Write-Host "Searching in:"
    Write-Host "1. $Path1"
    Write-Host "2. $Path2"
    Write-Host "3. $Path3"
    Write-Host "`nPlease run 'cargo build' first!"
    Read-Host "Press Enter to exit..."
    exit
}

Write-Host "Found exe at: $ExePath" -ForegroundColor Gray

# Apply Firewall Rule
try {
    Remove-NetFirewallRule -DisplayName $RuleName -ErrorAction SilentlyContinue
    
    New-NetFirewallRule -DisplayName $RuleName `
                        -Direction Inbound `
                        -Program $ExePath `
                        -Action Allow `
                        -Protocol Any `
                        -Profile Any `
                        -ErrorAction Stop | Out-Null

    Write-Host "SUCCESS! Firewall configured." -ForegroundColor Green
} catch {
    Write-Host "FAILED: $_" -ForegroundColor Red
    Write-Host "Please Right-Click > Run as Administrator" -ForegroundColor Yellow
}

Read-Host "Press Enter to exit..."