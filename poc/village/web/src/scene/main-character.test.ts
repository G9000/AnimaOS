import * as THREE from 'three';
import { afterEach, expect, it, vi } from 'vitest';
import { mainCharacterFrame, makeMainCharacter } from './main-character';

afterEach(() => vi.restoreAllMocks());

it('maps all four game directions to the supplied idle and walking rows', () => {
  for (const [direction, row] of [
    [0, 0],
    [1, 3],
    [2, 1],
    [3, 2],
  ]) {
    expect(mainCharacterFrame(direction!, false, 0)).toEqual({
      x: 3,
      y: row! * 48 + 6,
      width: 26,
      height: 34,
    });
    expect(mainCharacterFrame(direction!, true, 0)).toEqual({
      x: 3,
      y: (row! + 4) * 48 + 6,
      width: 26,
      height: 34,
    });
  }
});

it('uses the original 300ms idle and four-beat walking sequence without stepping through blank cells', () => {
  expect(
    [0, 0.3, 0.6, 0.9, 1.2].map((time) => mainCharacterFrame(0, true, time).x),
  ).toEqual([3, 35, 67, 35, 3]);
  expect(
    [0, 0.3, 0.6].map((time) => mainCharacterFrame(0, false, time).x),
  ).toEqual([3, 35, 3]);
});

it('uses one original crisp texture, switches motion safely, freezes on pause and disposes it once', () => {
  const texture = new THREE.Texture();
  const load = vi
    .spyOn(THREE.TextureLoader.prototype, 'load')
    .mockReturnValue(texture);
  const scene = new THREE.Scene();
  const player = makeMainCharacter(scene);
  expect(load).toHaveBeenCalledWith('/art/mc/spritesheet.png');
  expect(texture.magFilter).toBe(THREE.NearestFilter);
  expect(texture.minFilter).toBe(THREE.NearestFilter);
  expect(texture.colorSpace).toBe(THREE.SRGBColorSpace);
  player.animate(0.3, false, false);
  expect(texture.offset.x).toBeCloseTo(35 / 96);
  player.animate(0.3, true, false);
  expect(texture.offset.x).toBeCloseTo(3 / 96);
  player.animate(0.3, true, false);
  expect(texture.offset.x).toBeCloseTo(35 / 96);
  const before = texture.offset.clone();
  player.animate(1, true, true);
  expect(texture.offset).toEqual(before);
  player.animate(0.1, false, false);
  expect(texture.offset.x).toBeCloseTo(3 / 96);
  player.face(2);
  expect(texture.offset.y).toBeCloseTo((384 - 54 - 34) / 384);
  const dispose = vi.spyOn(texture, 'dispose');
  player.dispose();
  expect(dispose).toHaveBeenCalledOnce();
  expect(scene.children).toHaveLength(0);
});
