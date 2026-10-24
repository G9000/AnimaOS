import * as THREE from 'three';

const cropRows = [
  [
    [205, 23, 181, 416],
    [576, 18, 172, 423],
    [1012, 18, 172, 421],
    [1401, 18, 168, 423],
  ],
  [
    [172, 456, 250, 417],
    [575, 456, 195, 419],
    [973, 456, 250, 416],
    [1376, 457, 193, 418],
  ],
];

export function makeCharacter(row: number, scene: THREE.Scene) {
  const crops =
    row === 2
      ? [
          [225, 19, 275, 687],
          [725, 19, 247, 688],
          [1203, 19, 258, 688],
          [1709, 19, 233, 688],
        ]
      : cropRows[row]!;
  const textures = crops.map(([x, y, width, height]) => {
    const map = new THREE.TextureLoader().load(
      row === 2 ? '/art/mira-sprites.png' : '/art/characters.png',
    );
    map.colorSpace = THREE.SRGBColorSpace;
    map.magFilter = THREE.NearestFilter;
    map.minFilter = THREE.NearestFilter;
    const sheetWidth = row === 2 ? 2172 : 1774;
    const sheetHeight = row === 2 ? 724 : 887;
    map.repeat.set(width! / sheetWidth, height! / sheetHeight);
    map.offset.set(x! / sheetWidth, (sheetHeight - y! - height!) / sheetHeight);
    return map;
  });
  return makeBillboard(
    textures,
    row === 0 ? 0.88 : row === 2 ? 0.85 : 1.15,
    scene,
  );
}

export function makeBillboard(
  initialTextures: THREE.Texture[],
  width: number,
  scene: THREE.Scene,
) {
  const textures = initialTextures;
  const material = new THREE.SpriteMaterial({
    map: textures[0],
    transparent: true,
    alphaTest: 0.12,
    depthWrite: false,
    // Characters are foreground billboards: facade geometry must not clip their tilted sprite quads.
    depthTest: false,
    toneMapped: false,
  });
  const sprite = new THREE.Sprite(material);
  sprite.center.set(0.5, 0);
  sprite.scale.set(width, 2.05, 1);
  scene.add(sprite);
  const shadow = new THREE.Mesh(
    new THREE.CircleGeometry(0.38, 20),
    new THREE.MeshBasicMaterial({
      color: '#16201b',
      transparent: true,
      opacity: 0.22,
      depthWrite: false,
    }),
  );
  shadow.rotation.x = -Math.PI / 2;
  shadow.position.y = 0.025;
  scene.add(shadow);
  return {
    sprite,
    face(value: number) {
      material.map = textures[value];
      material.needsUpdate = true;
    },
    idle(time: number, paused: boolean, arranging = false) {
      const breath = paused ? 0 : Math.sin(time * 1.8 + (arranging ? 2 : 0));
      sprite.scale.y = 2.05 + breath * 0.018;
      material.rotation = paused
        ? 0
        : Math.sin(time * (arranging ? 0.9 : 1.3)) *
          (arranging ? 0.025 : 0.016);
    },
    place(x: number, z: number, bob = 0) {
      sprite.renderOrder = 100 + z;
      sprite.position.set(x, 0.04 + bob, z);
      shadow.position.set(x, 0.025, z);
    },
    dispose() {
      textures.forEach((texture) => texture.dispose());
      material.dispose();
      shadow.geometry.dispose();
      (shadow.material as THREE.Material).dispose();
      scene.remove(sprite, shadow);
    },
  };
}
