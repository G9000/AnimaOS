import * as THREE from 'three';
import { makeBillboard } from './characters';

const atlas = { width: 96, height: 384, cellWidth: 32, cellHeight: 48 };
const directionRows = [0, 3, 1, 2]; // Game: front, left, back, right.

/** Original source pixels share a fixed crop so backpack and feet never wobble between cells. */
export function mainCharacterFrame(
  direction: number,
  walking: boolean,
  seconds: number,
) {
  const sequence = walking ? [0, 1, 2, 1] : [0, 1];
  const elapsed = Number.isFinite(seconds) ? Math.max(0, seconds) : 0;
  const column =
    sequence[Math.floor((elapsed + 1e-9) / 0.3) % sequence.length]!;
  const row = (directionRows[direction] ?? 0) + (walking ? 4 : 0);
  return {
    x: column * atlas.cellWidth + 3,
    y: row * atlas.cellHeight + 6,
    width: 26,
    height: 34,
  };
}

export function makeMainCharacter(scene: THREE.Scene) {
  const texture = new THREE.TextureLoader().load('/art/mc/spritesheet.png');
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.magFilter = THREE.NearestFilter;
  texture.minFilter = THREE.NearestFilter;
  texture.generateMipmaps = false;
  texture.repeat.set(26 / atlas.width, 34 / atlas.height);
  const character = makeBillboard([texture], (26 / 34) * 2.05, scene);
  character.sprite.center.y = 1 / 34; // One source pixel of transparent padding below the boots.
  let direction = 0;
  let walking = false;
  let elapsed = 0;
  let lastFrame = '';
  const update = () => {
    const frame = mainCharacterFrame(direction, walking, elapsed);
    const key = `${frame.x}:${frame.y}`;
    if (key === lastFrame) return;
    lastFrame = key;
    texture.offset.set(
      frame.x / atlas.width,
      (atlas.height - frame.y - frame.height) / atlas.height,
    );
  };
  update();
  return {
    sprite: character.sprite,
    place: character.place,
    dispose: character.dispose,
    face(value: number) {
      direction = directionRows[value] === undefined ? 0 : value;
      update();
    },
    animate(delta: number, moving: boolean, paused: boolean) {
      if (paused) return;
      if (moving !== walking) {
        walking = moving;
        elapsed = 0;
      } else if (Number.isFinite(delta)) elapsed += Math.max(0, delta);
      update();
    },
  };
}
