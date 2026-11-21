# Shared drawing helpers for draw-icon.ps1 and draw-tray.ps1 (System.Drawing, Windows).
Add-Type -AssemblyName System.Drawing

function C([int]$a, [int]$r, [int]$g, [int]$b) { [Drawing.Color]::FromArgb($a, $r, $g, $b) }

function New-Canvas([int]$n) {
    $b = [Drawing.Bitmap]::new($n, $n, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [Drawing.Graphics]::FromImage($b); $g.Clear([Drawing.Color]::Transparent); $g.Dispose()
    $b
}

function Set-Quality($g) {
    $g.SmoothingMode = 'AntiAlias'; $g.InterpolationMode = 'HighQualityBicubic'
    $g.PixelOffsetMode = 'HighQuality'; $g.CompositingQuality = 'HighQuality'
}

# Linear gradient. GDI+ repeats a gradient past its end points, so callers span the whole area they
# fill; TileFlipXY at least mirrors instead of jumping if a shape pokes past the end.
function New-Linear([double]$x0, [double]$y0, [double]$x1, [double]$y1, $c0, $c1) {
    $b = [Drawing.Drawing2D.LinearGradientBrush]::new([Drawing.PointF]::new($x0, $y0), [Drawing.PointF]::new($x1, $y1), $c0, $c1)
    $b.WrapMode = [Drawing.Drawing2D.WrapMode]::TileFlipXY
    $b
}

function New-Squircle([double]$cx, [double]$cy, [double]$r, [double]$n = 5) {
    $p = [Drawing.Drawing2D.GraphicsPath]::new()
    $pts = for ($i = 0; $i -lt 720; $i++) {
        $t = $i * [math]::PI / 360; $c = [math]::Cos($t); $s = [math]::Sin($t)
        [Drawing.PointF]::new($cx + $r * [math]::Sign($c) * [math]::Pow([math]::Abs($c), 2 / $n), $cy + $r * [math]::Sign($s) * [math]::Pow([math]::Abs($s), 2 / $n))
    }
    $p.AddPolygon([Drawing.PointF[]]$pts); $p
}

# Radial glow: colour at the centre fading to transparent at radius r. $core: fraction kept solid.
function Fill-Glow($g, [double]$cx, [double]$cy, [double]$r, $color, [double]$core = 0) {
    $p = [Drawing.Drawing2D.GraphicsPath]::new(); $p.AddEllipse($cx - $r, $cy - $r * 0.62, 2 * $r, 2 * $r * 0.62)
    $b = [Drawing.Drawing2D.PathGradientBrush]::new($p)
    $b.CenterColor = $color; $b.SurroundColors = @([Drawing.Color]::FromArgb(0, $color))
    if ($core -gt 0) { $b.FocusScales = [Drawing.PointF]::new($core, $core) }
    $g.FillPath($b, $p)
}

function Shade($c, [double]$f) {
    [Drawing.Color]::FromArgb($c.A, [int]($c.R * $f), [int]($c.G * $f), [int]($c.B * $f))
}

# One isometric slab centred at (cx, cy): top face rhombus w x h, sides of thickness t.
function Draw-Slab($g, [double]$cx, [double]$cy, [double]$w, [double]$h, [double]$t, $top, $base) {
    $l = [Drawing.PointF]::new($cx - $w / 2, $cy); $r = [Drawing.PointF]::new($cx + $w / 2, $cy)
    $u = [Drawing.PointF]::new($cx, $cy - $h / 2); $d = [Drawing.PointF]::new($cx, $cy + $h / 2)
    $l2 = [Drawing.PointF]::new($l.X, $l.Y + $t); $r2 = [Drawing.PointF]::new($r.X, $r.Y + $t); $d2 = [Drawing.PointF]::new($d.X, $d.Y + $t)
    # left and right sides
    $g.FillPolygon((New-Linear $l.X $l.Y $d.X $d2.Y (Shade $base 0.80) (Shade $base 0.62)), [Drawing.PointF[]]@($l, $d, $d2, $l2))
    $g.FillPolygon((New-Linear $d.X $d.Y $r.X $r2.Y (Shade $base 0.55) (Shade $base 0.42)), [Drawing.PointF[]]@($d, $r, $r2, $d2))
    # top face: light from the back
    $g.FillPolygon((New-Linear $u.X $u.Y $d.X $d.Y $top $base), [Drawing.PointF[]]@($l, $u, $r, $d))
}

function Save-Png($bmp, [string]$path) {
    $full = [IO.Path]::GetFullPath($path)
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($full)) | Out-Null
    $bmp.Save($full, [Drawing.Imaging.ImageFormat]::Png)
}

# Downscale a large canvas to n x n with high-quality filtering.
function Resize-Bitmap($bmp, [int]$n) {
    $small = New-Canvas $n
    $g = [Drawing.Graphics]::FromImage($small); Set-Quality $g
    $g.DrawImage($bmp, 0, 0, $n, $n); $g.Dispose()
    $small
}
