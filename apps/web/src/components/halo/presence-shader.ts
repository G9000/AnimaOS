/** The orb's fragment shader, shared with design/shaders/presence.glsl. */
export const PRESENCE_FRAGMENT = `
precision highp float;
uniform vec2 u_resolution;
uniform float u_time;
uniform vec3 u_accent;
uniform vec3 u_core;
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
  vec2 uv = (frag - 0.5 * u_resolution) / min(u_resolution.x, u_resolution.y);

  float t = u_time * 0.12;
  vec2 q = vec2(fbm(uv * 2.2 + t), fbm(uv * 2.2 - t + 4.1));
  vec2 w = uv + 0.22 * (q - 0.5) * (0.6 + u_energy);
  float r = length(w);

  float body = smoothstep(0.48, 0.02, r);
  float texture = fbm(w * 3.4 + q * 2.0 + t * 1.5);
  float rings = 0.5 + 0.5 * sin(r * 46.0 - u_time * 1.4);
  float field = body * (0.55 + 0.6 * texture) + rings * body * 0.12 * u_energy;
  field = clamp(field, 0.0, 1.0);

  float th = bayer2(0.125 * cell) * 0.015625 + bayer2(0.25 * cell) * 0.0625
    + bayer2(0.5 * cell) * 0.25 + bayer2(cell);
  th = fract(th);

  float outer = step(th + 0.004, field * 1.15);
  float inner = step(th, pow(field, 2.2) * 1.6 - 0.15);

  vec3 col = mix(u_accent, u_core, inner);
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
