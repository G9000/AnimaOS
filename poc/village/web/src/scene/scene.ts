import * as THREE from 'three';
import {
  movePlayer,
  nearestNpc,
  npcApproach,
  npcPositions,
  type Position,
} from '../movement';
import type { BreadPreference, NpcId, VillagePhase } from '../types';
import { makeCharacter } from './characters';
import { makeMainCharacter } from './main-character';
import { buildWorld } from './world';

export interface VillageSceneHandle {
  pause: (paused: boolean) => void;
  approach: (npc: NpcId) => void;
  phase: (phase: VillagePhase) => void;
  preference: (preference: BreadPreference, collected: boolean) => void;
  dispose: () => void;
}

export function createVillageScene(
  canvas: HTMLCanvasElement,
  onNear: (near: NpcId | null) => void,
  onInteract: () => void,
): VillageSceneHandle {
  const renderer = new THREE.WebGLRenderer({
    canvas,
    antialias: false,
    alpha: false,
  });
  renderer.setPixelRatio(1);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.25;
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFShadowMap;
  const scene = new THREE.Scene();
  scene.background = new THREE.Color('#283e38');
  scene.fog = new THREE.FogExp2('#283e38', 0.026);
  const camera = new THREE.OrthographicCamera(-12, 12, 8, -8, 0.1, 80);
  camera.position.set(0, 15, 19);
  camera.lookAt(0, 0.8, 0.7);
  const sky = new THREE.HemisphereLight('#ebdec1', '#254335', 2.5);
  scene.add(sky);
  const sun = new THREE.DirectionalLight('#ffd39a', 2.1);
  sun.position.set(-8, 15, 8);
  sun.castShadow = true;
  sun.shadow.mapSize.set(1024, 1024);
  sun.shadow.camera.left = -14;
  sun.shadow.camera.right = 14;
  sun.shadow.camera.top = 12;
  sun.shadow.camera.bottom = -12;
  sun.shadow.normalBias = 0.04;
  scene.add(sun);
  const world = buildWorld(scene);
  world.preference(null, false);
  const player = makeMainCharacter(scene);
  const tavi = makeCharacter(0, scene);
  tavi.place(npcPositions.tavi.x, npcPositions.tavi.z);
  const mira = makeCharacter(2, scene);
  mira.place(npcPositions.mira.x, npcPositions.mira.z);
  const keys = new Set<string>();
  let paused = false;
  let position: Position = { x: 0, z: 5.2 };
  let destination: Position | null = null;
  let lastNear: NpcId | null = null;
  let previousTick = performance.now();
  let frame = 0;
  let dead = false;
  let time = 0;
  const raycaster = new THREE.Raycaster();
  const floor = new THREE.Plane(new THREE.Vector3(0, 1, 0), 0);
  const onKeyDown = (event: KeyboardEvent) => {
    if (
      paused ||
      event.ctrlKey ||
      event.metaKey ||
      event.altKey ||
      (event.target instanceof HTMLElement &&
        event.target.closest('input,textarea,select,[contenteditable=true]'))
    )
      return;
    const key = event.key.toLowerCase();
    if (
      [
        'arrowup',
        'arrowdown',
        'arrowleft',
        'arrowright',
        'w',
        'a',
        's',
        'd',
        'e',
      ].includes(key)
    ) {
      event.preventDefault();
      keys.add(key);
      destination = null;
    }
    if (key === 'e' && !event.repeat && nearestNpc(position)) onInteract();
  };
  const onKeyUp = (event: KeyboardEvent) => {
    keys.delete(event.key.toLowerCase());
  };
  const onBlur = () => {
    keys.clear();
    destination = null;
  };
  const onPointer = (event: PointerEvent) => {
    if (paused) return;
    canvas.focus({ preventScroll: true });
    const rect = canvas.getBoundingClientRect();
    raycaster.setFromCamera(
      new THREE.Vector2(
        ((event.clientX - rect.left) / rect.width) * 2 - 1,
        (-(event.clientY - rect.top) / rect.height) * 2 + 1,
      ),
      camera,
    );
    const hit = new THREE.Vector3();
    if (raycaster.ray.intersectPlane(floor, hit))
      destination = {
        x: Math.max(-7.2, Math.min(7.2, hit.x)),
        z: Math.max(1.4, Math.min(8.3, hit.z)),
      };
  };
  window.addEventListener('keydown', onKeyDown);
  window.addEventListener('keyup', onKeyUp);
  window.addEventListener('blur', onBlur);
  canvas.addEventListener('pointerdown', onPointer);
  const resize = () => {
    const width = canvas.clientWidth;
    const height = canvas.clientHeight;
    if (!width || !height) return;
    const halfHeight = width / height < 1.2 ? 9 : 7.2;
    camera.left = (-halfHeight * width) / height;
    camera.right = (halfHeight * width) / height;
    camera.top = halfHeight;
    camera.bottom = -halfHeight;
    camera.updateProjectionMatrix();
    renderer.setSize(
      Math.max(1, Math.floor(width / 1.45)),
      Math.max(1, Math.floor(height / 1.45)),
      false,
    );
  };
  const observer = new ResizeObserver(resize);
  observer.observe(canvas);
  resize();
  const animate = () => {
    if (dead) return;
    const tick = performance.now();
    const delta = Math.max(0, (tick - previousTick) / 1000);
    previousTick = tick;
    time += delta;
    let direction = {
      x:
        Number(keys.has('d') || keys.has('arrowright')) -
        Number(keys.has('a') || keys.has('arrowleft')),
      z:
        Number(keys.has('s') || keys.has('arrowdown')) -
        Number(keys.has('w') || keys.has('arrowup')),
    };
    if (destination && !paused) {
      const dx = destination.x - position.x;
      const dz = destination.z - position.z;
      if (Math.hypot(dx, dz) < 0.12) destination = null;
      else direction = { x: dx, z: dz };
    }
    const previousPosition = position;
    position = movePlayer(position, direction, delta, paused);
    const moving =
      !paused &&
      Math.hypot(
        position.x - previousPosition.x,
        position.z - previousPosition.z,
      ) > 0.00001;
    if (!paused && Math.hypot(direction.x, direction.z) > 0)
      player.face(
        Math.abs(direction.x) > Math.abs(direction.z)
          ? direction.x < 0
            ? 1
            : 3
          : direction.z < 0
            ? 2
            : 0,
      );
    player.place(position.x, position.z);
    player.animate(delta, moving, paused);
    const near = nearestNpc(position);
    if (near !== lastNear) {
      lastNear = near;
      onNear(near);
    }
    world.update(time);
    tavi.idle(time, paused);
    mira.idle(time, paused, true);
    renderer.render(scene, camera);
    frame = requestAnimationFrame(animate);
  };
  animate();
  return {
    pause(value) {
      paused = value;
      keys.clear();
      destination = null;
      if (value) player.animate(0, false, false);
    },
    approach(npc) {
      if (!paused) destination = npcApproach(npc);
    },
    phase(value) {
      const evening = value !== 'day';
      sky.color.set(evening ? '#a8a5d1' : '#ebdec1');
      sky.intensity = evening ? 1.15 : 2.5;
      sun.color.set(evening ? '#e4a36d' : '#ffd39a');
      sun.intensity = evening ? 0.65 : 2.1;
      scene.background = new THREE.Color(evening ? '#202f40' : '#283e38');
      (scene.fog as THREE.FogExp2).color.copy(scene.background);
      renderer.toneMappingExposure = evening ? 1.05 : 1.25;
      world.phase(value);
    },
    preference(value, collected) {
      world.preference(value, collected);
    },
    dispose() {
      dead = true;
      cancelAnimationFrame(frame);
      observer.disconnect();
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('blur', onBlur);
      canvas.removeEventListener('pointerdown', onPointer);
      player.dispose();
      tavi.dispose();
      mira.dispose();
      world.dispose();
      scene.traverse((object) => {
        if (object instanceof THREE.Mesh) {
          object.geometry.dispose();
          const list = Array.isArray(object.material)
            ? object.material
            : [object.material];
          list.forEach((value) => value.dispose());
        }
      });
      renderer.dispose();
    },
  };
}
