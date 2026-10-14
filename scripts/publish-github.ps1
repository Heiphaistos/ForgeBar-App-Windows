<#
.SYNOPSIS
    Crée le dépôt GitHub forge-bar et y pousse le projet.
.DESCRIPTION
    Nécessite Git et GitHub CLI (winget install GitHub.cli), puis `gh auth login`.
.EXAMPLE
    .\scripts\publish-github.ps1 -Visibility private
#>
param(
    [ValidateSet("private", "public")]
    [string]$Visibility = "private",
    [string]$Name = "forge-bar"
)
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

foreach ($tool in "git", "gh") {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "$tool est introuvable. Installez-le puis relancez le script."
    }
}
gh auth status | Out-Null

if (-not (Test-Path .git)) {
    git init -b main | Out-Null
    git add .
    git commit -m "Jalon 1 : socle Forge Bar (AppBar, rendu Direct2D, config, 50 thèmes, HUD Claude Code)" | Out-Null
}

$owner = gh api user --jq .login
(Get-Content Cargo.toml) -replace "github.com/OWNER/", "github.com/$owner/" | Set-Content Cargo.toml
if (git status --porcelain) {
    git commit -am "Dépôt : $owner/$Name" | Out-Null
}

gh repo create "$Name" "--$Visibility" --source . --push `
    --description "Barre d'extension Windows légère avec raccourcis système et HUD de consommation Claude"
Write-Host "Dépôt publié : https://github.com/$owner/$Name" -ForegroundColor Green
