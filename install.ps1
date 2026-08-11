$ErrorActionPreference = 'Stop'

$repo   = 'songaira/pomocard-cli'
$dir    = "$env:LOCALAPPDATA\pomocard"
$asset  = 'pomocard.exe'
$dest   = "$dir\$asset"

New-Item -ItemType Directory -Force -Path $dir | Out-Null
Write-Host "Installing pomocard-cli -> $dest"

# Prefer `gh` (handles auth, including private repos)
if (Get-Command gh -ErrorAction SilentlyContinue) {
    gh release download -R $repo -p $asset --output $dest --clobber
} else {
    # Fallback to the public GitHub API (fails on private repos)
    $rel  = Invoke-RestMethod 'https://api.github.com/repos/{0}/releases/latest' -f $repo
    $url  = ($rel.assets | Where-Object { $_.name -eq $asset }).browser_download_url
    if (-not $url) {
        Write-Error "No '$asset' in the latest release. Build from source instead: cargo install --path ."
        exit 1
    }
    Invoke-WebRequest $url -OutFile $dest
}

# Add to the user PATH (does not affect already-open terminals)
$paths = [Environment]::GetEnvironmentVariable('Path', 'User') -split ';'
if ($dir -notin $paths) {
    [Environment]::SetEnvironmentVariable('Path', ($paths + $dir) -join ';', 'User')
    Write-Host "Added $dir to your user PATH. Restart your terminal to use 'pomocard'."
}

Write-Host "Done. Run: pomocard"
