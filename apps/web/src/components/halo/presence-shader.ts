/** The orb's fragment shader, shared with design/shaders/presence.glsl (textureSize swapped for u_image_size so it runs on WebGL1). */
export const PRESENCE_FRAGMENT = `
precision highp float;
uniform vec2 u_resolution;
uniform float u_time;
uniform vec3 u_accent;
uniform vec3 u_core;
uniform vec3 u_accent2;
uniform float u_mix;
uniform float u_zoom;
uniform float u_cx;
uniform float u_cy;
uniform sampler2D u_image;
uniform vec2 u_image_size;
uniform float u_reveal;
uniform float u_img_zoom;
uniform float u_img_cx;
uniform float u_img_cy;
uniform float u_img_contrast;
uniform float u_img_mask;
uniform float u_pixel;
uniform float u_energy;
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
float bayer2(vec2 a) {
  a = floor(a);
  return fract(a.x / 2.0 + a.y * a.y * 0.75);
}
void main() {
  vec2 cell = floor(gl_FragCoord.xy / u_pixel);
  vec2 frag = (cell + 0.5) * u_pixel;
  float zoom = u_zoom > 0.0 ? u_zoom : 1.0;
  vec2 center = (u_cx == 0.0 && u_cy == 0.0) ? vec2(0.5) : vec2(u_cx, u_cy);
  vec2 uv = (frag - center * u_resolution) / (min(u_resolution.x, u_resolution.y) * zoom);
  float t = u_time * 0.12;
  vec2 q = vec2(fbm(uv * 2.2 + t), fbm(uv * 2.2 - t + 4.1));
  vec2 w = uv + 0.22 * (q - 0.5) * (0.6 + u_energy);
  float r = length(w);
  float body = smoothstep(0.48, 0.02, r);
  float texture = fbm(w * 3.4 + q * 2.0 + t * 1.5);
  float rings = 0.5 + 0.5 * sin(r * 46.0 - u_time * 1.4);
  float field = body * (0.55 + 0.6 * texture) + rings * body * 0.12 * u_energy;
  field = clamp(field, 0.0, 1.0);
  if (u_reveal > 0.001) {
    vec2 ts = u_image_size;
    float an = u_resolution.x / u_resolution.y;
    float ai = ts.x / max(ts.y, 1.0);
    vec2 ip = frag / u_resolution - 0.5;
    if (an > ai) ip.y *= ai / an; else ip.x *= an / ai;
    float iz = u_img_zoom > 0.0 ? u_img_zoom : 1.0;
    vec2 iuv = ip / iz + vec2(u_img_cx, 1.0 - u_img_cy);
    float inside = step(0.0, iuv.x) * step(iuv.x, 1.0) * step(0.0, iuv.y) * step(iuv.y, 1.0);
    vec3 ic = texture2D(u_image, iuv).rgb;
    float lum = dot(ic, vec3(0.299, 0.587, 0.114));
    float k = u_img_contrast > 0.0 ? u_img_contrast : 1.0;
    float con = clamp((lum - 0.5) * k + 0.5, 0.0, 1.0);
    float shape = smoothstep(0.66, 0.3, r);
    float imgField = smoothstep(0.1, 1.15, con) * 0.92 * inside * mix(1.0, shape, u_img_mask);
    float grain = fbm(cell * 0.045 + vec2(3.1, 8.7) + t * 0.6);
    float local = clamp((u_reveal * 1.7 - 0.35 - grain) * 3.0, 0.0, 1.0);
    float stray = step(0.985, hash(cell + floor(u_time * 2.0))) * step(0.02, imgField) * (1.0 - local) * u_reveal;
    field = max(mix(field, imgField, local), stray);
  }
  float th = bayer2(0.125 * cell) * 0.015625 + bayer2(0.25 * cell) * 0.0625 + bayer2(0.5 * cell) * 0.25 + bayer2(cell);
  th = fract(th);
  float outer = step(th + 0.004, field * 1.15);
  float inner = step(th, pow(field, 2.2) * 1.6 - 0.15);
  float tone = fbm(w * 1.6 + vec2(7.3, 2.1) - t * 0.5);
  vec3 edge = mix(u_accent, u_accent2, step(1.0 - u_mix * 0.6, tone + 0.15) * step(0.001, u_mix));
  vec3 col = mix(edge, u_core, inner);
  float a = outer;
  gl_FragColor = vec4(col * a, a);
}
`;

export const PRESENCE_VERTEX = `
attribute vec2 a_position;
void main() {
  gl_Position = vec4(a_position, 0.0, 1.0);
}
`;
