import { useEffect, useRef, useState } from 'react';
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
export function HaloOrb({
  accent = '#E94DAB',
  core = '#F4F3EF',
  energy = 0.4,
  pixel = 1.5,
  className = '',
}: {
  accent?: string;
  core?: string;
  energy?: number;
  pixel?: number;
  className?: string;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const uniforms = useRef({ accent, core, energy, pixel });
  uniforms.current = { accent, core, energy, pixel };
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
    };
  }, []);

  // A still frame under reduced motion still has to show colour changes.
  useEffect(() => {
    if (!reducedMotion()) return;
    canvasRef.current?.dispatchEvent(new Event('halo-redraw'));
  }, [accent, energy]);

  return (
    <div
      className={`halo-orb ${className}`}
      data-motion="agent-orb"
      data-fallback={fallback || undefined}
      style={{ ['--halo-accent' as string]: accent }}
      aria-hidden
    >
      {!fallback && <canvas ref={canvasRef} />}
    </div>
  );
}
