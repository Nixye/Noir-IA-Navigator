[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$destination = Join-Path $projectRoot 'apps/browser-desktop/assets'
New-Item -ItemType Directory -Path $destination -Force | Out-Null

# Original vector artwork. The raster export below uses the same coordinates,
# paths and palette; it does not use a font or an external image asset.
@'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256">
  <title>Noir</title>
  <defs>
    <linearGradient id="surface" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#293632"/>
      <stop offset="1" stop-color="#0e1214"/>
    </linearGradient>
    <linearGradient id="letter" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#e9f7c4"/>
      <stop offset="1" stop-color="#c3db92"/>
    </linearGradient>
  </defs>
  <rect x="12" y="12" width="232" height="232" rx="50" fill="url(#surface)"/>
  <rect x="12" y="12" width="232" height="232" rx="50" fill="none" stroke="#78927d" stroke-opacity=".35" stroke-width="2"/>
  <path d="M72 176V80L184 176V80" fill="none" stroke="url(#letter)" stroke-width="17" stroke-linecap="round" stroke-linejoin="round"/>
  <circle cx="72" cy="57" r="4.5" fill="#dbefaa" fill-opacity=".6"/>
  <circle cx="184" cy="199" r="4.5" fill="#dbefaa" fill-opacity=".6"/>
</svg>
'@ | Set-Content -LiteralPath (Join-Path $destination 'noir.svg') -Encoding utf8

Add-Type -AssemblyName System.Drawing
function New-RoundedIconPath {
    $path = [Drawing.Drawing2D.GraphicsPath]::new()
    $path.AddArc(12,12,100,100,180,90)
    $path.AddArc(144,12,100,100,270,90)
    $path.AddArc(144,144,100,100,0,90)
    $path.AddArc(12,144,100,100,90,90)
    $path.CloseFigure()
    $path
}

$large = [Drawing.Bitmap]::new(1024,1024,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
$graphics = [Drawing.Graphics]::FromImage($large)
try {
    $graphics.Clear([Drawing.Color]::Transparent)
    $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $graphics.ScaleTransform(4,4)
    $path = New-RoundedIconPath
    $surface = [Drawing.Drawing2D.LinearGradientBrush]::new([Drawing.PointF]::new(12,12),[Drawing.PointF]::new(244,244),[Drawing.Color]::FromArgb(41,54,50),[Drawing.Color]::FromArgb(14,18,20))
    $rim = [Drawing.Pen]::new([Drawing.Color]::FromArgb(89,120,146,125),2)
    $letter = [Drawing.Drawing2D.LinearGradientBrush]::new([Drawing.PointF]::new(72,57),[Drawing.PointF]::new(184,199),[Drawing.Color]::FromArgb(233,247,196),[Drawing.Color]::FromArgb(195,219,146))
    $pen = [Drawing.Pen]::new($letter,17)
    $dots = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(153,219,239,170))
    try {
        $graphics.FillPath($surface,$path)
        $graphics.DrawPath($rim,$path)
        $pen.StartCap = [Drawing.Drawing2D.LineCap]::Round
        $pen.EndCap = [Drawing.Drawing2D.LineCap]::Round
        $pen.LineJoin = [Drawing.Drawing2D.LineJoin]::Round
        $points = [Drawing.PointF[]]@([Drawing.PointF]::new(72,176),[Drawing.PointF]::new(72,80),[Drawing.PointF]::new(184,176),[Drawing.PointF]::new(184,80))
        $graphics.DrawLines($pen,$points)
        $graphics.FillEllipse($dots,67.5,52.5,9,9)
        $graphics.FillEllipse($dots,179.5,194.5,9,9)
    } finally { $path.Dispose(); $surface.Dispose(); $rim.Dispose(); $letter.Dispose(); $pen.Dispose(); $dots.Dispose() }
} finally { $graphics.Dispose() }

$entries = [Collections.Generic.List[object]]::new()
try {
    foreach ($size in @(16,32,48,64,128,256)) {
        $bitmap = [Drawing.Bitmap]::new($size,$size,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $resampler = [Drawing.Graphics]::FromImage($bitmap)
        try {
            $resampler.Clear([Drawing.Color]::Transparent)
            $resampler.CompositingMode = [Drawing.Drawing2D.CompositingMode]::SourceCopy
            $resampler.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $resampler.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
            $resampler.DrawImage($large,[Drawing.Rectangle]::new(0,0,$size,$size),0,0,1024,1024,[Drawing.GraphicsUnit]::Pixel)
        } finally { $resampler.Dispose() }
        try {
            $png = Join-Path $destination "noir-$size.png"
            $bitmap.Save($png,[Drawing.Imaging.ImageFormat]::Png)
            # Standard 32-bit DIB entries with an AND mask for all sizes,
            # including 256px, so native icon readers need no PNG extension.
            $bytes = & {
                $stream = [IO.MemoryStream]::new()
                $writer = [IO.BinaryWriter]::new($stream)
                try {
                    $maskStride = [int]([Math]::Ceiling($size / 32.0) * 4)
                    $writer.Write([int]40); $writer.Write([int]$size); $writer.Write([int]($size*2))
                    $writer.Write([uint16]1); $writer.Write([uint16]32); $writer.Write([int]0)
                    $writer.Write([int]($size*$size*4)); $writer.Write([int]0); $writer.Write([int]0); $writer.Write([int]0); $writer.Write([int]0)
                    for ($y=$size-1; $y -ge 0; $y--) { for ($x=0; $x -lt $size; $x++) { $pixel=$bitmap.GetPixel($x,$y); $writer.Write([byte]$pixel.B); $writer.Write([byte]$pixel.G); $writer.Write([byte]$pixel.R); $writer.Write([byte]$pixel.A) } }
                    for ($y=$size-1; $y -ge 0; $y--) {
                        $mask = [byte[]]::new($maskStride)
                        for ($x=0; $x -lt $size; $x++) { if ($bitmap.GetPixel($x,$y).A -eq 0) { $mask[[int][Math]::Floor($x/8)] = $mask[[int][Math]::Floor($x/8)] -bor (128 -shr ($x%8)) } }
                        $writer.Write($mask)
                    }
                    $writer.Flush(); $stream.ToArray()
                } finally { $writer.Dispose(); $stream.Dispose() }
            }
            $entries.Add([pscustomobject]@{Size=$size;Data=$bytes})
        } finally { $bitmap.Dispose() }
    }
} finally { $large.Dispose() }

$iconStream = [IO.File]::Create((Join-Path $destination 'noir.ico'))
$iconWriter = [IO.BinaryWriter]::new($iconStream)
try {
    $iconWriter.Write([uint16]0); $iconWriter.Write([uint16]1); $iconWriter.Write([uint16]$entries.Count)
    $offset = 6 + 16 * $entries.Count
    foreach ($entry in $entries) {
        $dimension = if ($entry.Size -eq 256) { 0 } else { $entry.Size }
        $iconWriter.Write([byte]$dimension); $iconWriter.Write([byte]$dimension); $iconWriter.Write([byte]0); $iconWriter.Write([byte]0)
        $iconWriter.Write([uint16]1); $iconWriter.Write([uint16]32); $iconWriter.Write([uint32]$entry.Data.Length); $iconWriter.Write([uint32]$offset)
        $offset += $entry.Data.Length
    }
    foreach ($entry in $entries) { $iconWriter.Write([byte[]]$entry.Data) }
} finally { $iconWriter.Dispose(); $iconStream.Dispose() }
Write-Output "Generated original Noir icon assets: $destination"
