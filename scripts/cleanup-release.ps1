# Limpia assets no deseados del release v0.1.2
# Uso: .\scripts\cleanup-release.ps1 -Tag "v0.1.2" -Token "ghp_xxx"

param($Tag = "v0.1.2", $Token)

$repo = "ROSALDEV-SAC/yola-desktop"
$releaseUrl = "https://api.github.com/repos/$repo/releases/tags/$Tag"
$headers = @{ Authorization = "Bearer $Token"; Accept = "application/vnd.github+json" }

$release = Invoke-RestMethod -Uri $releaseUrl -Headers $headers
Write-Host "Release: $($release.name) - $($release.assets.Count) assets"

$toDelete = $release.assets | Where-Object { 
    $_.name -match '\.so\.|\.so$|copyright|\.png$|data\.tar|im-' -and $_.name -notmatch 'yola-desktop\.png'
}

Write-Host "Assets a eliminar: $($toDelete.Count)"
foreach ($asset in $toDelete) {
    Invoke-RestMethod -Uri $asset.url -Method Delete -Headers $headers
    Write-Host "Eliminado: $($asset.name)"
}
Write-Host "Listo."
