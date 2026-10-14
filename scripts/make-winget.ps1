<#
.SYNOPSIS
    Génère les trois fichiers du manifeste winget (Heiphaistos.ForgeBar) pour une version publiée.
.EXAMPLE
    .\scripts\make-winget.ps1 -Version 0.2.0 -ShaX64 <hash> -ShaX86 <hash> -Out dist\winget
.NOTES
    Le dépôt microsoft/winget-pkgs reçoit le dossier généré par une pull request, relue par ses mainteneurs.
#>
param(
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$ShaX64,
    [Parameter(Mandatory)][string]$ShaX86,
    [string]$Out = "dist/winget",
    [string]$Repo = "Heiphaistos/ForgeBar-App-Windows"
)
$ErrorActionPreference = "Stop"
$id = "Heiphaistos.ForgeBar"
$dir = Join-Path $Out "manifests/h/Heiphaistos/ForgeBar/$Version"
New-Item -ItemType Directory -Force $dir | Out-Null
$base = "https://github.com/$Repo/releases/download/v$Version"

@"
PackageIdentifier: $id
PackageVersion: $Version
DefaultLocale: fr-FR
ManifestType: version
ManifestVersion: 1.6.0
"@ | Set-Content (Join-Path $dir "$id.yaml") -Encoding utf8

@"
PackageIdentifier: $id
PackageVersion: $Version
InstallerLocale: fr-FR
MinimumOSVersion: 10.0.17763.0
InstallerType: wix
Scope: user
UpgradeBehavior: install
Installers:
- Architecture: x64
  InstallerUrl: $base/ForgeBar-$Version-x64.msi
  InstallerSha256: $($ShaX64.ToUpper())
- Architecture: x86
  InstallerUrl: $base/ForgeBar-$Version-x86.msi
  InstallerSha256: $($ShaX86.ToUpper())
ManifestType: installer
ManifestVersion: 1.6.0
"@ | Set-Content (Join-Path $dir "$id.installer.yaml") -Encoding utf8

@"
PackageIdentifier: $id
PackageVersion: $Version
PackageLocale: fr-FR
Publisher: Heiphaistos
PackageName: Forge Bar
PackageUrl: https://github.com/$Repo
License: MIT
LicenseUrl: https://github.com/$Repo/blob/main/LICENSE
ShortDescription: Barre d'extension Windows légère avec raccourcis système, monitoring et HUD de consommation Claude.
Moniker: forgebar
Tags:
- barre
- widgets
- claude
- monitoring
ManifestType: defaultLocale
ManifestVersion: 1.6.0
"@ | Set-Content (Join-Path $dir "$id.locale.fr-FR.yaml") -Encoding utf8
Write-Host "Manifeste écrit dans $dir"
