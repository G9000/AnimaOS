precision highp float;

/** @resolution */
uniform vec2 u_resolution;

/** @time */
uniform float u_time;

/**
 * @label Mode (0 dots, 1 halftone, 2 mosaic, 3 threshold, 4 color halftone)
 * @default 0
 * @range 0, 4
 */
uniform float u_mode;

/**
 * @label Accent
 * @color
 * @default #E94DAB
 */
uniform vec3 u_accent;

/**
 * @label Second accent
 * @color
 * @default #8C93FF
 */
uniform vec3 u_accent2;

/**
 * @label Core
 * @color
 * @default #ECE6DA
 */
uniform vec3 u_core;

/**
 * @label Second accent amount
 * @default 0.35
 * @range 0, 1
 */
uniform float u_mix;

/**
 * @label Pixel Size
 * @default 4
 * @range 1, 16
 */
uniform float u_pixel;

/**
 * @label Energy
 * @default 0.55
 * @range 0, 1
 */
uniform float u_energy;

/**
 * @label Zoom
 * @default 1
 * @range 0.3, 3
 */
uniform float u_zoom;

/**
 * @label Center X
 * @default 0.5
 * @range 0, 1
 */
uniform float u_cx;

/**
 * @label Center Y
 * @default 0.5
 * @range 0, 1
 */
uniform float u_cy;

float hash(vec2 p) {
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
             mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
}

float fbm(vec2 p) {
  float v = 0.0;
  float a = 0.5;
  for (int i = 0; i < 5; i++) {
    v += a * noise(p);
    p = p * 2.03 + vec2(1.7, 9.2);
    a *= 0.5;
  }
  return v;
}

vec2 rot(vec2 p, float a) {
  float c = cos(a);
  float s = sin(a);
  return vec2(c * p.x - s * p.y, s * p.x + c * p.y);
}

float fieldAt(vec2 frag, out float tone) {
  vec2 center = vec2(u_cx, u_cy) * u_resolution;
  vec2 uv = (frag - center) / (min(u_resolution.x, u_resolution.y) * u_zoom);
  float t = u_time * 0.12;
  vec2 q = vec2(fbm(uv * 2.2 + t), fbm(uv * 2.2 - t + 4.1));
  vec2 w = uv + 0.22 * (q - 0.5) * (0.6 + u_energy);
  float r = length(w);
  float body = smoothstep(0.48, 0.02, r);
  float tex = fbm(w * 3.4 + q * 2.0 + t * 1.5);
  float rings = 0.5 + 0.5 * sin(r * 46.0 - u_time * 1.4);
  tone = fbm(w * 1.6 + vec2(7.3, 2.1) - t * 0.5);
  return clamp(body * (0.55 + 0.6 * tex) + rings * body * 0.12 * u_energy, 0.0, 1.0);
}

vec3 hue(float tone) {
  return mix(u_accent, u_accent2, smoothstep(0.62 - u_mix * 0.3, 0.66 - u_mix * 0.3, tone));
}

float ht(vec2 p, float s, float a, float k) {
  vec2 rp = rot(p, a);
  vec2 c = (floor(rp / s) + 0.5) * s;
  float tone;
  float f = fieldAt(rot(c, -a), tone);
  float d = length(rp - c) / s;
  float rad = 0.5 * sqrt(clamp(f * k, 0.0, 1.0));
  return smoothstep(rad, rad - 0.12, d);
}

void main() {
  vec2 p = gl_FragCoord.xy;
  int m = int(u_mode + 0.5);
  vec3 col = vec3(0.0);
  float a = 0.0;
  float tone;

  if (m == 0) {
    float s = u_pixel * 2.0;
    vec2 cell = floor(p / s);
    vec2 local = fract(p / s);
    float f = fieldAt((cell + 0.5) * s, tone);
    float dotSq = step(abs(local.x - 0.5), 0.2) * step(abs(local.y - 0.5), 0.2);
    float on = step(hash(cell), f * 1.9 - 0.04);
    float stray = step(0.992, hash(cell + 3.7)) * step(0.02, f + 0.04);
    a = dotSq * max(on, stray);
    col = mix(u_core, u_accent2, step(0.62 - u_mix * 0.3, tone));
    col = mix(col, u_accent, step(0.975, hash(cell + 9.1)));
  } else if (m == 1) {
    float s = u_pixel * 2.5;
    vec2 cell = floor(p / s);
    vec2 local = fract(p / s);
    float f = fieldAt((cell + 0.5) * s, tone);
    float rad = 0.5 * sqrt(f) * 1.05;
    a = smoothstep(rad, rad - 0.1, length(local - 0.5));
    col = mix(hue(tone), u_core, smoothstep(0.6, 0.9, f));
  } else if (m == 2) {
    float s = u_pixel * 6.0;
    vec2 cell = floor(p / s);
    float f = fieldAt((cell + 0.5) * s, tone);
    float fq = floor(f * 5.0) / 4.0;
    a = step(0.1, f);
    col = mix(hue(tone) * (0.35 + 0.65 * fq), u_core, smoothstep(0.7, 1.0, fq) * 0.7);
  } else if (m == 3) {
    float s = max(1.0, u_pixel * 0.5);
    vec2 cell = floor(p / s);
    float f = fieldAt((cell + 0.5) * s, tone);
    float n = hash(cell + floor(u_time * 8.0));
    a = step(n, pow(f, 1.4) * 1.25);
    col = mix(u_accent, u_core, smoothstep(0.25, 0.5, f));
  } else {
    float s = u_pixel * 2.2;
    float c1 = ht(p, s, 0.26, 1.0);
    float c2 = ht(p, s, 0.79, 0.85);
    float c3 = ht(p, s, 1.31, 0.6);
    col = u_accent * c1 + u_accent2 * c2 + u_core * c3 * 0.8;
    a = clamp(max(max(c1, c2), c3), 0.0, 1.0);
    col = clamp(col, 0.0, 1.0);
    if (a > 0.0) col = col / max(a, 0.001);
  }

  gl_FragColor = vec4(col * a, a);
}
