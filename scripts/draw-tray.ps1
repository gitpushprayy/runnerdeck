#requires -Version 7.2
# Draws the tray icon sprites into src-tauri/icons/tray/ (32x32 PNGs, drawn at 8x and scaled down).
# The app composes them at runtime (src-tauri/src/icon.rs): three layers (one per runner, lit while that
# runner is busy) plus a status badge in the corner. Windows only (System.Drawing).
param([string]$OutDir = (Join-Path $PSScriptRoot '..\src-tauri\icons\tray'))
. (Join-Path $PSScriptRoot 'drawing.ps1')

$S = 8; $Canvas = 32 * $S
function Save-Sprite([scriptblock]$draw, [string]$name) {
    $big = New-Canvas $Canvas; $g = [Drawing.Graphics]::FromImage($big); Set-Quality $g
    & $draw $g
    $g.Dispose()
    $small = Resize-Bitmap $big 32; Save-Png $small (Join-Path $OutDir "$name.png"); $small.Dispose(); $big.Dispose()
}

# stack geometry (in 32px units), leaving the bottom-right corner for the badge
$cx = 14.5; $w = 25; $h = 12.5; $t = 3.2
$layers = @(
    @{ Cy = 8.0; Top = (C 255 147 197 253); Base = (C 255 37 99 235) }     # 1: blue   (GitHub 1)
    @{ Cy = 14.0; Top = (C 255 216 180 254); Base = (C 255 139 92 246) }   # 2: violet (GitHub 2)
    @{ Cy = 20.0; Top = (C 255 94 234 212); Base = (C 255 13 148 136) }    # 3: teal   (TeamCity)
)
$dimTop = C 255 176 184 198; $dimBase = C 255 110 118 134

for ($i = 0; $i -lt 3; $i++) {
    $l = $layers[$i]; $num = $i + 1
    Save-Sprite { param($g) Draw-Slab $g ($cx * $S) ($l.Cy * $S) ($w * $S) ($h * $S) ($t * $S) $l.Top $l.Base }.GetNewClosure() "layer-$num-lit"
    Save-Sprite { param($g) Draw-Slab $g ($cx * $S) ($l.Cy * $S) ($w * $S) ($h * $S) ($t * $S) $dimTop $dimBase }.GetNewClosure() "layer-$num-dim"
}

# badges: a dot with a dark ring so it reads on light and dark taskbars
$bx = 25.0 * $S; $by = 25.0 * $S; $br = 5.6 * $S; $ring = 1.4 * $S
function Draw-Badge($g, $fill, [string]$glyph, [double]$halo) {
    if ($halo -gt 0) {
        $hr = $br + $ring + $halo * $S
        $g.FillEllipse([Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(110, $fill)), $bx - $hr, $by - $hr, 2 * $hr, 2 * $hr)
    }
    $or = $br + $ring
    $g.FillEllipse([Drawing.SolidBrush]::new((C 255 11 15 25)), $bx - $or, $by - $or, 2 * $or, 2 * $or)
    $g.FillEllipse((New-Linear $bx ($by - $br) $bx ($by + $br) ([Drawing.Color]::FromArgb(255, [math]::Min(255, $fill.R + 50), [math]::Min(255, $fill.G + 50), [math]::Min(255, $fill.B + 50))) $fill), $bx - $br, $by - $br, 2 * $br, 2 * $br)
    $white = [Drawing.SolidBrush]::new((C 255 255 255 255))
    switch ($glyph) {
        'play' { $g.FillPolygon($white, [Drawing.PointF[]]@([Drawing.PointF]::new($bx - 1.7 * $S, $by - 2.6 * $S), [Drawing.PointF]::new($bx + 2.8 * $S, $by), [Drawing.PointF]::new($bx - 1.7 * $S, $by + 2.6 * $S))) }
        'bang' { $g.FillRectangle($white, $bx - 0.75 * $S, $by - 3.2 * $S, 1.5 * $S, 4.0 * $S); $g.FillEllipse($white, $bx - 0.85 * $S, $by + 1.6 * $S, 1.7 * $S, 1.7 * $S) }
        'cross' {
            $p = [Drawing.Pen]::new((C 255 255 255 255), 1.6 * $S); $p.StartCap = 'Round'; $p.EndCap = 'Round'
            $d = 2.2 * $S; $g.DrawLine($p, $bx - $d, $by - $d, $bx + $d, $by + $d); $g.DrawLine($p, $bx - $d, $by + $d, $bx + $d, $by - $d)
        }
    }
}
Save-Sprite { param($g) Draw-Badge $g (C 255 18 161 80) '' 0 } 'badge-idle'
Save-Sprite { param($g) Draw-Badge $g (C 255 47 111 237) 'play' 0 } 'badge-busy'
Save-Sprite { param($g) Draw-Badge $g (C 255 47 111 237) 'play' 1.4 } 'badge-busy-pulse'
Save-Sprite { param($g) Draw-Badge $g (C 255 217 119 6) 'bang' 0 } 'badge-warn'
Save-Sprite { param($g) Draw-Badge $g (C 255 220 38 38) 'cross' 0 } 'badge-down'
"tray sprites written to $OutDir"
