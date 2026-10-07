# Post or replace one GitHub issue comment from a UTF-8 file, byte-for-byte.
#
# Windows PowerShell 5.1 cannot be trusted with this by default: `Invoke-RestMethod -Body <string>`
# encodes the body as ISO-8859-1, so every non-ASCII character reaches GitHub as `?`. The tracker's
# written evidence is Chinese, so the body is sent as explicit UTF-8 bytes with the charset stated.
param(
    [Parameter(Mandatory)][int]$Issue,
    [Parameter(Mandatory)][string]$BodyFile,
    # Replace an existing comment instead of adding one, so a corrupted body can be corrected in place.
    [long]$Replace = 0
)
$ErrorActionPreference = 'Stop'
$token = "protocol=https`nhost=github.com`n`n" | git credential fill 2>$null |
    Select-String -Pattern '^password=' | ForEach-Object { $_.Line.Substring(9) }
if (-not $token) { throw 'No GitHub credential is available.' }
$body = [System.IO.File]::ReadAllText((Resolve-Path -LiteralPath $BodyFile).Path, [System.Text.Encoding]::UTF8)
$bytes = [System.Text.Encoding]::UTF8.GetBytes((ConvertTo-Json -Compress -InputObject @{ body = $body }))
$client = New-Object System.Net.WebClient
$client.Headers.Add('Authorization', "token $token")
$client.Headers.Add('User-Agent', 'dsh-agent')
$client.Headers.Add('Accept', 'application/vnd.github+json')
# `charset=utf-8` matters: without it GitHub may assume the payload is not UTF-8 and reject it.
$client.Headers.Add('Content-Type', 'application/json; charset=utf-8')
$uri = if ($Replace -gt 0) {
    "https://api.github.com/repos/T-miracle/Editor/issues/comments/$Replace"
} else {
    "https://api.github.com/repos/T-miracle/Editor/issues/$Issue/comments"
}
try {
    $reply = $client.UploadData($uri, 'POST', $bytes)
} finally {
    $client.Dispose()
}
$posted = [System.Text.Encoding]::UTF8.GetString($reply) | ConvertFrom-Json

# Read it back and count what GitHub actually stored, so a mangled body cannot pass unnoticed.
$verify = New-Object System.Net.WebClient
$verify.Headers.Add('Authorization', "token $token")
$verify.Headers.Add('User-Agent', 'dsh-agent')
$verify.Headers.Add('Accept', 'application/vnd.github+json')
$stored = [System.Text.Encoding]::UTF8.GetString(
    $verify.DownloadData("https://api.github.com/repos/T-miracle/Editor/issues/comments/$($posted.id)")) |
    ConvertFrom-Json
$verify.Dispose()
$cjk = 0
foreach ($ch in $stored.body.ToCharArray()) {
    if ([int]$ch -ge 0x4E00 -and [int]$ch -le 0x9FFF) { $cjk++ }
}
$expected = 0
foreach ($ch in $body.ToCharArray()) {
    if ([int]$ch -ge 0x4E00 -and [int]$ch -le 0x9FFF) { $expected++ }
}
"comment {0} on #{1}: {2} CJK characters stored of {3} sent, {4} bytes" -f `
    $posted.id, $Issue, $cjk, $expected, $stored.body.Length
if ($cjk -ne $expected) { throw 'The stored comment does not match what was sent.' }
