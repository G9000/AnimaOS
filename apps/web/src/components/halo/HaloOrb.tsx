import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { PRESENCE_FRAGMENT, PRESENCE_VERTEX } from './presence-shader';

function rgb(hex: string): [number, number, number] {
  const value = parseInt(hex.replace('#', ''), 16);
  return [
    ((value >> 16) & 255) / 255,
    ((value >> 8) & 255) / 255,
    (value & 255) / 255,
  ];
}

function reducedMotion(): boolean {
  return (
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  );
}

function compile(gl: WebGLRenderingContext, type: number, source: string) {
  const shader = gl.createShader(type);
  if (!shader) return null;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    gl.deleteShader(shader);
    return null;
  }
  return shader;
}

/**
 * anima's presence: the pixel orb from design/shaders/presence.glsl, drawn
 * with WebGL. Energy and colour change with what anima is doing. Without
 * WebGL it falls back to a soft gradient; with reduced motion it holds still.
 */
export interface HaloPortrait {
  /** Image the orb dissolves into. */
  src: string;
  /** 0 is the orb, 1 is the portrait. */
  reveal: number;
  zoom?: number;
  cx?: number;
  cy?: number;
  contrast?: number;
  /** 1 fades the portrait out at the orb's edge; 0 fills the whole frame. */
  mask?: number;
}

export function HaloOrb({
  accent = '#E94DAB',
  core = '#F4F3EF',
  accent2 = '#8C93FF',
  mix = 0,
  zoom = 1,
  cx = 0.5,
  cy = 0.5,
  energy = 0.4,
  pixel = 1.5,
  portrait,
  className = '',
  style,
}: {
  accent?: string;
  core?: string;
  /** A second colour mixed lightly into the orb's edge. */
  accent2?: string;
  mix?: number;
  zoom?: number;
  cx?: number;
  cy?: number;
  energy?: number;
  pixel?: number;
  portrait?: HaloPortrait;
  className?: string;
  style?: CSSProperties;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const uniforms = useRef({
    accent,
    core,
    accent2,
    mix,
    zoom,
    cx,
    cy,
    energy,
    pixel,
    portrait,
  });
  uniforms.current = {
    accent,
    core,
    accent2,
    mix,
    zoom,
    cx,
    cy,
    energy,
    pixel,
    portrait,
  };
  const portraitSrc = portrait?.src;
  const [fallback, setFallback] = useState(false);

  useEffect(() => {
    const canvas = canvasRef.current;
    let gl: WebGLRenderingContext | null = null;
    try {
      gl = canvas?.getContext('webgl', { premultipliedAlpha: true }) ?? null;
    } catch {
      gl = null;
    }
    if (!canvas || !gl) {
      setFallback(true);
      return;
    }
    const vs = compile(gl, gl.VERTEX_SHADER, PRESENCE_VERTEX);
    const fs = compile(gl, gl.FRAGMENT_SHADER, PRESENCE_FRAGMENT);
    const program = gl.createProgram();
    if (!vs || !fs || !program) {
      setFallback(true);
      return;
    }
    gl.attachShader(program, vs);
    gl.attachShader(program, fs);
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      setFallback(true);
      return;
    }
    gl.useProgram(program);
    const buffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(
      gl.ARRAY_BUFFER,
      new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]),
      gl.STATIC_DRAW,
    );
    const position = gl.getAttribLocation(program, 'a_position');
    gl.enableVertexAttribArray(position);
    gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);
    const at = (name: string) => gl!.getUniformLocation(program, name);
    const loc = {
      resolution: at('u_resolution'),
      time: at('u_time'),
      accent: at('u_accent'),
      core: at('u_core'),
      pixel: at('u_pixel'),
      energy: at('u_energy'),
      accent2: at('u_accent2'),
      mix: at('u_mix'),
      zoom: at('u_zoom'),
      cx: at('u_cx'),
      cy: at('u_cy'),
      image: at('u_image'),
      imageSize: at('u_image_size'),
      reveal: at('u_reveal'),
      imgZoom: at('u_img_zoom'),
      imgCx: at('u_img_cx'),
      imgCy: at('u_img_cy'),
      imgContrast: at('u_img_contrast'),
      imgMask: at('u_img_mask'),
    };

    // The portrait is one texture, uploaded once it has loaded. Until then
    // the reveal stays at zero and the orb draws on its own.
    const texture = gl.createTexture();
    let imageSize: [number, number] = [1, 1];
    let imageReady = false;
    let loadedSrc: string | undefined;
    let image: HTMLImageElement | null = null;
    const loadPortrait = (src: string | undefined) => {
      if (src === loadedSrc) return;
      loadedSrc = src;
      imageReady = false;
      image = null;
      if (!src || !texture) return;
      const next = new Image();
      next.onload = () => {
        if (loadedSrc !== src) return;
        gl!.activeTexture(gl!.TEXTURE0);
        gl!.bindTexture(gl!.TEXTURE_2D, texture);
        gl!.pixelStorei(gl!.UNPACK_FLIP_Y_WEBGL, 1);
        gl!.texImage2D(
          gl!.TEXTURE_2D,
          0,
          gl!.RGBA,
          gl!.RGBA,
          gl!.UNSIGNED_BYTE,
          next,
        );
        gl!.texParameteri(
          gl!.TEXTURE_2D,
          gl!.TEXTURE_WRAP_S,
          gl!.CLAMP_TO_EDGE,
        );
        gl!.texParameteri(
          gl!.TEXTURE_2D,
          gl!.TEXTURE_WRAP_T,
          gl!.CLAMP_TO_EDGE,
        );
        gl!.texParameteri(gl!.TEXTURE_2D, gl!.TEXTURE_MIN_FILTER, gl!.LINEAR);
        gl!.texParameteri(gl!.TEXTURE_2D, gl!.TEXTURE_MAG_FILTER, gl!.LINEAR);
        imageSize = [next.naturalWidth, next.naturalHeight];
        imageReady = true;
        image = next;
        canvas.dispatchEvent(new Event('halo-redraw'));
      };
      next.src = src;
    };

    const still = reducedMotion();
    let frame = 0;
    let smoothed = uniforms.current.energy;
    const start = performance.now();
    const draw = (now: number) => {
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      const width = Math.round(canvas.clientWidth * ratio);
      const height = Math.round(canvas.clientHeight * ratio);
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
      }
      const u = uniforms.current;
      // Energy eases toward its target so state changes feel alive, not abrupt.
      smoothed += (u.energy - smoothed) * 0.06;
      gl!.viewport(0, 0, width, height);
      gl!.clearColor(0, 0, 0, 0);
      gl!.clear(gl!.COLOR_BUFFER_BIT);
      gl!.uniform2f(loc.resolution, width, height);
      gl!.uniform1f(loc.time, still ? 4 : (now - start) / 1000);
      gl!.uniform3f(loc.accent, ...rgb(u.accent));
      gl!.uniform3f(loc.core, ...rgb(u.core));
      gl!.uniform1f(loc.pixel, u.pixel * ratio);
      gl!.uniform1f(loc.energy, still ? u.energy : smoothed);
      gl!.uniform3f(loc.accent2, ...rgb(u.accent2));
      gl!.uniform1f(loc.mix, u.mix);
      gl!.uniform1f(loc.zoom, u.zoom);
      gl!.uniform1f(loc.cx, u.cx);
      gl!.uniform1f(loc.cy, u.cy);
      loadPortrait(u.portrait?.src);
      const p = u.portrait;
      gl!.activeTexture(gl!.TEXTURE0);
      gl!.bindTexture(gl!.TEXTURE_2D, texture);
      gl!.uniform1i(loc.image, 0);
      gl!.uniform2f(loc.imageSize, imageSize[0], imageSize[1]);
      gl!.uniform1f(loc.reveal, p && imageReady && image ? p.reveal : 0);
      gl!.uniform1f(loc.imgZoom, p?.zoom ?? 1);
      gl!.uniform1f(loc.imgCx, p?.cx ?? 0.5);
      gl!.uniform1f(loc.imgCy, p?.cy ?? 0.5);
      gl!.uniform1f(loc.imgContrast, p?.contrast ?? 1);
      gl!.uniform1f(loc.imgMask, p?.mask ?? 1);
      gl!.drawArrays(gl!.TRIANGLE_STRIP, 0, 4);
      if (!still && !document.hidden) frame = requestAnimationFrame(draw);
    };
    const resume = () => {
      cancelAnimationFrame(frame);
      if (!document.hidden) frame = requestAnimationFrame(draw);
    };
    const redraw = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(draw);
    };
    frame = requestAnimationFrame(draw);
    document.addEventListener('visibilitychange', resume);
    canvas.addEventListener('halo-redraw', redraw);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener('visibilitychange', resume);
      canvas.removeEventListener('halo-redraw', redraw);
      gl!.deleteProgram(program);
      gl!.deleteShader(vs);
      gl!.deleteShader(fs);
      gl!.deleteBuffer(buffer);
      gl!.deleteTexture(texture);
    };
  }, []);

  // A still frame under reduced motion still has to show colour changes.
  useEffect(() => {
    if (!reducedMotion()) return;
    canvasRef.current?.dispatchEvent(new Event('halo-redraw'));
  }, [accent, energy, mix, zoom, cx, cy, portraitSrc, portrait?.reveal]);

  return (
    <div
      className={`halo-orb ${className}`}
      data-motion="agent-orb"
      data-fallback={fallback || undefined}
      style={{ ['--halo-accent' as string]: accent, ...style }}
      aria-hidden
    >
      {!fallback && <canvas ref={canvasRef} />}
    </div>
  );
}
