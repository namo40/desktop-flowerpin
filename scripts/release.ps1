<#
.SYNOPSIS
    공식 배포 파일을 만든다. 진짜 그림 팩과 그 키가 있는 PC에서만 된다.

.DESCRIPTION
    1. 커밋하지 않은 고침이 없는지 본다.
    2. 형식, clippy, 테스트를 보고, 공식 빌드 검사를 켜고 빌드한다. 개발 키나 대체 그림이면 빌드가 멈춘다.
    3. 실행 파일, 안내 글, 라이선스를 한 압축 파일로 묶고 SHA-256을 적는다.
    4. -Draft를 주면 GitHub에 릴리스 초안을 만든다. 초안은 살펴본 뒤 GitHub에서 직접 공개한다.
       공개할 때 그 커밋에 태그가 붙는다.

    결과는 target/dist 에 생긴다. 그림 팩을 어디서 찾는지는 README의 "그림"에 있다.

.EXAMPLE
    scripts/release.ps1
    묶기만 한다.

.EXAMPLE
    scripts/release.ps1 -Draft
    묶고, GitHub에 릴리스 초안을 만든다.
#>
param(
    # GitHub에 릴리스 초안을 만든다. gh가 로그인되어 있어야 하고, 지금 커밋이 올라가 있어야 한다.
    [switch]$Draft,
    # 커밋하지 않은 고침이 있어도 묶는다. 시험 삼아 묶어 볼 때만 쓴다. -Draft와 함께 쓸 수 없다.
    [switch]$AllowDirty
)

$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

# 명령이 실패하면 멈춘다.
function Invoke-Step([string]$what, [scriptblock]$command) {
    Write-Host "== $what"
    & $command
    if ($LASTEXITCODE -ne 0) { throw "${what}: 실패했습니다 (exit $LASTEXITCODE)" }
}

# ---------- 1. 버전과 저장소의 상태 ----------
$manifest = [System.IO.File]::ReadAllText((Join-Path (Get-Location) 'Cargo.toml'), [System.Text.Encoding]::UTF8)
if ($manifest -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'Cargo.toml에서 버전을 찾지 못했습니다' }
$version = $Matches[1]
$tag = "v$version"
$commit = (git rev-parse HEAD).Trim()
$short = (git rev-parse --short HEAD).Trim()

$dirty = @(git status --porcelain)
if ($dirty.Count -gt 0 -and -not $AllowDirty) {
    throw "커밋하지 않은 고침이 있습니다. 커밋한 뒤에 다시 돌려 주세요.`n$($dirty -join "`n")"
}
if ($Draft) {
    if ($AllowDirty) { throw '-Draft와 -AllowDirty는 함께 쓸 수 없습니다' }
    Invoke-Step '올라간 커밋 가져오기' { git fetch --quiet origin }
    $remote = (git rev-parse origin/main).Trim()
    if ($remote -ne $commit) { throw "지금 커밋($short)이 origin/main과 다릅니다. 먼저 올려 주세요." }
    cmd /c "gh release view $tag >nul 2>nul"
    if ($LASTEXITCODE -eq 0) { throw "$tag 릴리스가 이미 있습니다. 버전을 올리거나 그 릴리스를 지운 뒤에 다시 돌려 주세요." }
}
Write-Host "데스크탑 꽃핀 $version (커밋 $short)"

# ---------- 2. 확인과 빌드 ----------
# GitHub Actions는 빌드가 되는지만 본다. 형식, clippy, 테스트는 여기서 본다.
Invoke-Step '형식' { cargo fmt --check }
Invoke-Step 'clippy' { cargo clippy --all-targets --locked -- -D warnings }
Invoke-Step '테스트' { cargo test --locked }
$env:DFP_RELEASE = '1'
try {
    Invoke-Step '공식 빌드' { cargo build --release --locked }
} finally {
    Remove-Item Env:DFP_RELEASE
}

# ---------- 3. 묶기 ----------
$out = Join-Path (Get-Location) 'target\dist'
$stage = Join-Path $out 'DesktopFlowerpin'
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force $stage | Out-Null

Copy-Item 'target\release\DesktopFlowerpin.exe' $stage
Copy-Item 'LICENSE' (Join-Path $stage 'LICENSE.txt')
Copy-Item 'LICENSE-ASSETS.md' $stage
Copy-Item 'crates\app\res\fonts\LICENSE-NanumGothic.txt' $stage

# 틀의 빈칸을 채운다.
function Expand-Template([string]$path, [hashtable]$values) {
    $text = [System.IO.File]::ReadAllText($path, [System.Text.Encoding]::UTF8)
    foreach ($name in $values.Keys) { $text = $text.Replace("{{$name}}", $values[$name]) }
    $text.Replace("`r`n", "`n")
}

$name = "DesktopFlowerpin-$version-windows-x64"
$values = @{ VERSION = $version; COMMIT = $short; DATE = (Get-Date -Format 'yyyy-MM-dd'); ZIP = "$name.zip" }
$withBom = New-Object System.Text.UTF8Encoding $true
$withoutBom = New-Object System.Text.UTF8Encoding $false
# 안내 글은 메모장에서 한글이 깨지지 않게 BOM을 붙이고, 줄 끝을 Windows식으로 한다.
foreach ($guide in Get-ChildItem 'packaging' -Filter '*.txt') {
    $text = (Expand-Template $guide.FullName $values).Replace("`n", "`r`n")
    [System.IO.File]::WriteAllText((Join-Path $stage $guide.Name), $text, $withBom)
}

# 파일을 하나씩 넣는다. 폴더를 통째로 묶는 함수는 Windows PowerShell에서 경로를 역슬래시로 적어서,
# Windows 밖에서는 풀리지 않는 압축 파일이 된다.
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = Join-Path $out "$name.zip"
$level = [System.IO.Compression.CompressionLevel]::Optimal
$stream = [System.IO.File]::Open($zip, [System.IO.FileMode]::Create)
try {
    $mode = [System.IO.Compression.ZipArchiveMode]::Create
    $archive = New-Object System.IO.Compression.ZipArchive($stream, $mode, $false, [System.Text.Encoding]::UTF8)
    try {
        foreach ($file in Get-ChildItem $stage -File | Sort-Object Name) {
            $inside = "DesktopFlowerpin/$($file.Name)"
            [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $file.FullName, $inside, $level)
        }
    } finally {
        $archive.Dispose()
    }
} finally {
    $stream.Dispose()
}

$values.ZIP_SHA256 = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
$values.EXE_SHA256 = (Get-FileHash (Join-Path $stage 'DesktopFlowerpin.exe') -Algorithm SHA256).Hash.ToLower()
# 올리는 파일의 값만 적는다. 그래야 받은 자리에서 `sha256sum -c`로 바로 견줄 수 있다.
# 압축 파일 안에 든 실행 파일의 값은 릴리스 글에 적는다.
$sums = Join-Path $out 'SHA256SUMS.txt'
[System.IO.File]::WriteAllText($sums, "$($values.ZIP_SHA256)  $name.zip`n", $withoutBom)
$notes = Join-Path $out 'release-notes.md'
$notesFrame = Join-Path (Get-Location) 'packaging\release-notes.md'
[System.IO.File]::WriteAllText($notes, (Expand-Template $notesFrame $values), $withoutBom)

Write-Host ''
Get-ChildItem $stage | ForEach-Object { '{0,12:N0}  {1}' -f $_.Length, $_.Name } | Write-Host
'{0,12:N0}  -> {1}' -f (Get-Item $zip).Length, $zip | Write-Host
Write-Host "$($values.ZIP_SHA256)  $name.zip"
Write-Host "$($values.EXE_SHA256)  DesktopFlowerpin.exe"

# ---------- 4. 릴리스 초안 ----------
if ($Draft) {
    # 태그는 초안을 공개할 때 붙는다. 그사이에 main이 나아가도 이 커밋에 붙게 커밋을 적어 둔다.
    Invoke-Step '릴리스 초안 만들기' {
        gh release create $tag $zip $sums --draft --target $commit --title "데스크탑 꽃핀 $version" --notes-file $notes
    }
    Write-Host '초안을 만들었습니다. GitHub의 Releases에서 살펴본 뒤 공개해 주세요.'
} else {
    Write-Host '묶기만 했습니다. 릴리스 초안까지 만들려면 -Draft를 주세요.'
}
