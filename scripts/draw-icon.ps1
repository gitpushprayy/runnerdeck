#requires -Version 7.2
# Draws app-icon.png (1024x1024), the source for `npm run icons` (tauri icon). Windows only (System.Drawing).
# The mark: an isometric stack of three glowing layers, one per runner (blue, violet, teal, the same colours
# the dashboard uses), on a deep navy squircle. No text.
param([string]$Out = (Join-Path $PSScriptRoot '..\app-icon.png'))
. (Join-Path $PSScriptRoot 'drawing.ps1')

$N = 1024
$bmp = New-Canvas $N
$g = [Drawing.Graphics]::FromImage($bmp); Set-Quality $g

# ---- background: squircle, deep navy diagonal gradient, soft light top-left, faint bloom bottom-right
$bgPath = New-Squircle 512 512 452
$g.FillPath((New-Linear 0 0 0 1024 (C 255 30 41 74) (C 255 8 11 24)), $bgPath)
$g.SetClip($bgPath)
Fill-Glow $g 250 170 520 (C 70 110 140 255)
Fill-Glow $g 820 900 480 (C 46 140 90 255)
Fill-Glow $g 512 560 360 (C 70 80 130 255)

# ---- the stack (drawn bottom layer first)
$layers = @(
    @{ Cy = 640; Top = (C 255 94 234 212); Base = (C 255 13 148 136) }    # teal   (TeamCity)
    @{ Cy = 505; Top = (C 255 216 180 254); Base = (C 255 139 92 246) }   # violet (GitHub 2)
    @{ Cy = 370; Top = (C 255 147 197 253); Base = (C 255 37 99 235) }    # blue   (GitHub 1)
)
# a soft shadow pool under the stack
Fill-Glow $g 512 770 300 (C 120 0 0 0) 0.32
# coloured halos behind the layers (drawn first so they never wash over them)
foreach ($l in $layers) { Fill-Glow $g 512 ($l.Cy + 20) 380 ([Drawing.Color]::FromArgb(46, $l.Base)) }
foreach ($l in $layers) { Draw-Slab $g 512 $l.Cy 600 300 54 $l.Top $l.Base }
# a crisp highlight on the top face's front edges of the top layer
$top = $layers[2]
$pen = [Drawing.Pen]::new((C 150 255 255 255), 5); $pen.LineJoin = 'Round'
$g.DrawLines($pen, [Drawing.PointF[]]@([Drawing.PointF]::new(212, 370), [Drawing.PointF]::new(512, 220), [Drawing.PointF]::new(812, 370)))

# ---- glass sheen and inner edge
$sheen = New-Linear 0 0 0 1024 (C 24 255 255 255) (C 0 255 255 255)
$blend = [Drawing.Drawing2D.ColorBlend]::new(4)
$blend.Colors = @((C 26 255 255 255), (C 18 255 255 255), (C 0 255 255 255), (C 0 255 255 255)); $blend.Positions = @(0.0, 0.08, 0.34, 1.0)
$sheen.InterpolationColors = $blend
$g.FillPath($sheen, $bgPath)
$g.ResetClip()
$g.DrawPath([Drawing.Pen]::new((C 40 255 255 255), 3), (New-Squircle 512 512 450))

Save-Png $bmp $Out
$g.Dispose(); $bmp.Dispose()
"wrote $Out"
