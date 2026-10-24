import * as THREE from 'three';
import type { BreadPreference, VillagePhase } from '../types';

export function buildWorld(scene: THREE.Scene) {
  const materials = new Map<string, THREE.MeshStandardMaterial>();
  const material = (color: string) => {
    if (!materials.has(color))
      materials.set(
        color,
        new THREE.MeshStandardMaterial({
          color,
          roughness: 0.92,
          flatShading: true,
        }),
      );
    return materials.get(color)!;
  };
  const box = (
    x: number,
    y: number,
    z: number,
    w: number,
    h: number,
    d: number,
    color: string,
    parent: THREE.Object3D = scene,
  ) => {
    const mesh = new THREE.Mesh(
      new THREE.BoxGeometry(w, h, d),
      material(color),
    );
    mesh.position.set(x, y, z);
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    parent.add(mesh);
    return mesh;
  };
  const cylinder = (
    x: number,
    y: number,
    z: number,
    radius: number,
    height: number,
    color: string,
    parent: THREE.Object3D = scene,
  ) => {
    const mesh = new THREE.Mesh(
      new THREE.CylinderGeometry(radius * 0.86, radius, height, 8),
      material(color),
    );
    mesh.position.set(x, y, z);
    mesh.castShadow = true;
    parent.add(mesh);
    return mesh;
  };
  let seed = 41;
  const random = () => {
    seed = (seed * 16807) % 2147483647;
    return (seed - 1) / 2147483646;
  };
  const canvas = document.createElement('canvas');
  canvas.width = 256;
  canvas.height = 256;
  const context = canvas.getContext('2d')!;
  context.fillStyle = '#55564b';
  context.fillRect(0, 0, 256, 256);
  for (let row = 0; row < 8; row++)
    for (let col = -1; col < 7; col++) {
      const x = col * 44 + (row % 2) * 22;
      const y = row * 32;
      context.fillStyle = [
        '#aaa080',
        '#b4a98b',
        '#9d967c',
        '#c0b291',
        '#a79e81',
      ][Math.floor(random() * 5)]!;
      context.fillRect(x + 2, y + 2, 39, 27);
      context.fillStyle = '#c2b79b';
      context.fillRect(x + 4, y + 3, 34, 2);
      context.fillStyle = '#666c4b';
      if (random() > 0.7) context.fillRect(x + 1, y + 24, 10, 4);
    }
  const groundMap = new THREE.CanvasTexture(canvas);
  groundMap.wrapS = groundMap.wrapT = THREE.RepeatWrapping;
  groundMap.repeat.set(5, 5);
  groundMap.colorSpace = THREE.SRGBColorSpace;
  groundMap.magFilter = THREE.NearestFilter;
  const groundMat = new THREE.MeshStandardMaterial({
    map: groundMap,
    roughness: 1,
  });
  const ground = new THREE.Mesh(new THREE.PlaneGeometry(18, 19), groundMat);
  ground.rotation.x = -Math.PI / 2;
  ground.position.z = 1.7;
  ground.receiveShadow = true;
  scene.add(ground);
  box(0, -0.5, 0, 60, 0.8, 60, '#263c30');
  box(0, 1.6, -2.1, 10.4, 3.2, 4.4, '#625d46');
  box(0, 1.8, 0.12, 9.8, 2.8, 0.12, '#343c32');
  box(0, 1.45, 0.24, 3.15, 2.9, 0.16, '#1c2b27');
  for (const x of [-5, -3.4, -1.55, 1.55, 3.4, 5])
    box(x, 1.7, 0.36, 0.22, 3.45, 0.3, '#46392c');
  for (const y of [0.22, 2.75, 3.18])
    box(0, y, 0.39, 10.6, 0.19, 0.34, '#735133');
  for (const x of [-3.55, 3.55]) {
    box(x, 1.75, 0.26, 2.3, 1.25, 0.16, '#d59a49');
    const glass = new THREE.Mesh(
      new THREE.PlaneGeometry(2.05, 1.04),
      new THREE.MeshBasicMaterial({ color: '#f5c86f' }),
    );
    glass.position.set(x, 1.75, 0.36);
    scene.add(glass);
    for (const dx of [-0.7, 0, 0.7])
      box(x + dx, 1.75, 0.46, 0.065, 1.3, 0.12, '#4f3f2c');
    box(x, 1.75, 0.46, 2.35, 0.07, 0.12, '#4f3f2c');
    box(x, 0.96, 0.8, 2.7, 0.2, 1.05, '#725334');
    for (let loaf = 0; loaf < 5; loaf++) {
      const bread = new THREE.Mesh(
        new THREE.CapsuleGeometry(0.1, 0.23, 3, 6),
        material('#d9a053'),
      );
      bread.rotation.z = Math.PI / 2;
      bread.position.set(x - 0.9 + loaf * 0.44, 1.12, 0.8);
      scene.add(bread);
    }
  }
  const roofShape = new THREE.Shape();
  roofShape.moveTo(-3.05, 0);
  roofShape.lineTo(0, 1.8);
  roofShape.lineTo(3.05, 0);
  roofShape.closePath();
  const roof = new THREE.Mesh(
    new THREE.ExtrudeGeometry(roofShape, { depth: 11.2, bevelEnabled: false }),
    material('#344c47'),
  );
  roof.rotation.y = Math.PI / 2;
  roof.position.set(-5.6, 3.22, -2.0);
  roof.castShadow = true;
  scene.add(roof);
  const tiles = new THREE.InstancedMesh(
    new THREE.BoxGeometry(0.55, 0.07, 0.67),
    material('#ffffff'),
    200,
  );
  const matrix = new THREE.Matrix4();
  const tileRotation = new THREE.Quaternion();
  const roofAngle = Math.atan2(1.8, 3.05);
  let tileIndex = 0;
  for (const side of [-1, 1])
    for (let row = 0; row < 5; row++)
      for (let col = 0; col < 20; col++) {
        const distance = (row + 0.5) * 0.61;
        tileRotation.setFromEuler(new THREE.Euler(side * roofAngle, 0, 0));
        matrix.compose(
          new THREE.Vector3(
            -5.32 + col * 0.56 + (row % 2) * 0.05,
            3.28 + (1 - distance / 3.05) * 1.8,
            -2 + side * distance,
          ),
          tileRotation,
          new THREE.Vector3(1, 1, 1),
        );
        tiles.setMatrixAt(tileIndex, matrix);
        tiles.setColorAt(
          tileIndex,
          new THREE.Color(
            ['#43645c', '#4b6b60', '#3c5b53', '#567466'][
              Math.floor(random() * 4)
            ]!,
          ),
        );
        tileIndex++;
      }
  tiles.castShadow = true;
  tiles.receiveShadow = true;
  scene.add(tiles);
  box(0, 5.08, -2, 11.4, 0.16, 0.24, '#6b7760');
  for (let x = -5.5; x < 5.6; x += 0.55)
    box(x, 3.22, 1.08, 0.48, 0.16, 0.6, '#49615a');
  box(0, 3.3, 1.1, 11.35, 0.19, 0.17, '#725435');
  box(0, 2.93, 1.35, 3.0, 0.72, 0.2, '#3c4e42');
  const signCanvas = document.createElement('canvas');
  signCanvas.width = 512;
  signCanvas.height = 128;
  const signContext = signCanvas.getContext('2d')!;
  signContext.fillStyle = '#ead9a8';
  signContext.font = 'bold 58px Georgia';
  signContext.textAlign = 'center';
  signContext.fillText('TAVI’S', 256, 70);
  signContext.font = '18px Georgia';
  signContext.fillText('BREAD & LITTLE THINGS', 256, 103);
  const signMap = new THREE.CanvasTexture(signCanvas);
  signMap.colorSpace = THREE.SRGBColorSpace;
  const sign = new THREE.Mesh(
    new THREE.PlaneGeometry(2.35, 0.59),
    new THREE.MeshBasicMaterial({ map: signMap, transparent: true }),
  );
  sign.position.set(0, 2.93, 1.47);
  scene.add(sign);
  const lanterns: THREE.PointLight[] = [];
  let evening = false;
  for (const x of [-4.9, -1.75, 1.75, 4.9]) {
    box(x, 2.58, 0.85, 0.07, 0.65, 0.07, '#2b352c');
    const glow = new THREE.Mesh(
      new THREE.CylinderGeometry(0.16, 0.2, 0.42, 6),
      new THREE.MeshBasicMaterial({ color: '#ffd47d' }),
    );
    glow.position.set(x, 2.2, 0.85);
    scene.add(glow);
    for (const y of [1.98, 2.43]) box(x, y, 0.85, 0.43, 0.07, 0.39, '#4a3929');
    const light = new THREE.PointLight('#ffbd61', 8, 5, 1.8);
    light.position.set(x, 2.1, 1.2);
    scene.add(light);
    lanterns.push(light);
  }
  const plant = (x: number, z: number, size = 1) => {
    cylinder(x, 0.22 * size, z, 0.32 * size, 0.44 * size, '#9e6744');
    cylinder(x, 0.43 * size, z, 0.35 * size, 0.11 * size, '#b07a4c');
    for (let i = 0; i < 6; i++) {
      const leaf = new THREE.Mesh(
        new THREE.IcosahedronGeometry((0.25 + random() * 0.17) * size, 0),
        material(['#3d6d43', '#668b43', '#8c9c48'][i % 3]!),
      );
      leaf.position.set(
        x + (random() - 0.5) * size * 0.6,
        (0.7 + random() * 0.38) * size,
        z + (random() - 0.5) * size * 0.5,
      );
      leaf.scale.y = 1.3;
      scene.add(leaf);
    }
  };
  for (const [x, z, size] of [
    [-5.5, 1, 1.5],
    [5.4, 1.1, 1.5],
    [-6.2, 4, 1.7],
    [5.6, 5.2, 1.3],
    [-4.6, 6.7, 1.1],
    [6.8, 2.3, 1.2],
  ])
    plant(x!, z!, size!);
  for (let i = 0; i < 15; i++) {
    const x = (i < 8 ? -1 : 1) * (8 + random() * 3);
    const z = random() * 17 - 6;
    cylinder(x, 1, z, 0.21, 2, '#514b35');
    for (let layer = 0; layer < 3; layer++) {
      const crown = new THREE.Mesh(
        new THREE.ConeGeometry(1.7 - layer * 0.3, 2.4, 7),
        material(layer % 2 ? '#31503b' : '#294735'),
      );
      crown.position.set(x, 2 + layer, z);
      scene.add(crown);
    }
  }
  for (const x of [-6.5, 6.5]) {
    cylinder(x, 0.45, 2.6, 0.44, 0.9, '#785834');
    box(x, 0.95, 2.6, 0.8, 0.08, 0.8, '#a57b49');
  }
  box(-2.3, 0.35, 1.0, 1.45, 0.7, 0.9, '#7e5936');
  box(-2.3, 0.75, 1.0, 1.6, 0.12, 1.0, '#a67b49');
  const plate = new THREE.Group();
  plate.position.set(-2.3, 0.87, 1.0);
  scene.add(plate);
  const breadGroups = ['#d49b4d', '#a87641'].map((color, i) => {
    const group = new THREE.Group();
    group.position.x = i === 0 ? -0.32 : 0.32;
    plate.add(group);
    cylinder(0, 0, 0, 0.29, 0.04, '#d5c8a4', group);
    const loaf = new THREE.Mesh(
      new THREE.SphereGeometry(0.22, 8, 5),
      material(color),
    );
    loaf.scale.set(1.1, 0.55, 0.9);
    loaf.position.y = 0.08;
    group.add(loaf);
    return group;
  });

  // Warm timber and copper carry a few village-scale magical circuits.
  for (const x of [-4.72, 4.72]) {
    cylinder(x, 1.42, 0.63, 0.035, 2.5, '#ba815a');
    box(x > 0 ? 3.65 : -3.65, 0.32, 0.63, 2.14, 0.07, 0.07, '#ba815a');
  }
  const runeMaterial = new THREE.MeshBasicMaterial({
    color: '#76e2d1',
    transparent: true,
    opacity: 0.82,
  });
  const violetMaterial = new THREE.MeshBasicMaterial({
    color: '#c69df2',
    transparent: true,
    opacity: 0.7,
  });
  const runeLamps: THREE.PointLight[] = [];
  for (const [x, z] of [
    [-5.8, 3.0],
    [6.3, 5.6],
  ]) {
    cylinder(x!, 1.2, z!, 0.065, 2.4, '#8f6549');
    const cage = new THREE.Mesh(
      new THREE.OctahedronGeometry(0.24),
      x! < 0 ? runeMaterial : violetMaterial,
    );
    cage.position.set(x!, 2.5, z!);
    scene.add(cage);
    const ring = new THREE.Mesh(
      new THREE.TorusGeometry(0.3, 0.018, 4, 16),
      material('#ca9762'),
    );
    ring.position.copy(cage.position);
    ring.rotation.x = Math.PI / 2;
    scene.add(ring);
    const light = new THREE.PointLight(x! < 0 ? '#54e5d2' : '#bf8cff', 4, 5);
    light.position.copy(cage.position);
    scene.add(light);
    runeLamps.push(light);
  }
  // Mira lays out supper under a small rune sign, away from the shop counter.
  box(5.3, 0.78, 4.45, 2.25, 0.16, 1.25, '#9d7048');
  for (const x of [4.4, 6.2])
    for (const z of [4.05, 4.85]) box(x, 0.38, z, 0.13, 0.76, 0.13, '#614b36');
  box(5.3, 0.885, 4.45, 0.72, 0.035, 1.2, '#637b71');
  for (const x of [4.65, 5.95]) {
    cylinder(x, 0.9, 4.45, 0.25, 0.055, '#e6d7ac');
    cylinder(x, 1.04, 4.05, 0.1, 0.24, '#8caa9d');
  }
  const supperBread = new THREE.Group();
  supperBread.position.set(5.3, 0.94, 4.45);
  for (const dx of [-0.18, 0.18]) {
    const loaf = new THREE.Mesh(
      new THREE.CapsuleGeometry(0.12, 0.2, 3, 6),
      material('#d8a35b'),
    );
    loaf.rotation.z = Math.PI / 2;
    loaf.position.x = dx;
    supperBread.add(loaf);
  }
  scene.add(supperBread);
  supperBread.visible = false;
  const hologramCanvas = document.createElement('canvas');
  hologramCanvas.width = 256;
  hologramCanvas.height = 128;
  const holoContext = hologramCanvas.getContext('2d')!;
  holoContext.strokeStyle = '#8ce8db';
  holoContext.lineWidth = 3;
  holoContext.strokeRect(8, 8, 240, 112);
  holoContext.fillStyle = '#a4f6e6';
  holoContext.font = '24px Georgia';
  holoContext.textAlign = 'center';
  holoContext.fillText('✧  MIRA’S  ✧', 128, 53);
  holoContext.font = '17px Georgia';
  holoContext.fillText('SUPPER TABLE', 128, 88);
  const holoMap = new THREE.CanvasTexture(hologramCanvas);
  holoMap.colorSpace = THREE.SRGBColorSpace;
  const hologram = new THREE.Sprite(
    new THREE.SpriteMaterial({
      map: holoMap,
      transparent: true,
      opacity: 0.8,
      depthWrite: false,
      toneMapped: false,
    }),
  );
  hologram.position.set(5.4, 2.4, 4.2);
  hologram.scale.set(1.9, 0.95, 1);
  scene.add(hologram);
  const motes = Array.from({ length: 18 }, (_, i) => {
    const mote = new THREE.Mesh(
      new THREE.OctahedronGeometry(0.024 + random() * 0.018),
      i % 3 ? runeMaterial : violetMaterial,
    );
    const origin = {
      x: random() * 13 - 6.5,
      y: random() * 2 + 0.7,
      z: random() * 7 + 0.7,
    };
    mote.position.set(origin.x, origin.y, origin.z);
    scene.add(mote);
    return { mote, origin };
  });
  return {
    update(time: number) {
      lanterns.forEach((light, i) => {
        light.intensity =
          (evening ? 11 : 7.5) + Math.sin(time * 2.3 + i * 3) * 0.45;
      });
      runeLamps.forEach((light, i) => {
        light.intensity = (evening ? 5 : 2.2) + Math.sin(time + i * 2) * 0.3;
      });
      hologram.position.y = 2.4 + Math.sin(time * 0.8) * 0.06;
      motes.forEach(({ mote, origin }, i) => {
        mote.position.x = origin.x + Math.sin(time * 0.25 + i) * 0.2;
        mote.position.y = origin.y + Math.sin(time * 0.7 + i) * 0.18;
        mote.rotation.y = time * 0.4;
      });
    },
    phase(value: VillagePhase) {
      evening = value !== 'day';
      supperBread.visible = evening;
    },
    preference(preference: BreadPreference, collected: boolean) {
      plate.visible = collected;
      breadGroups[0]!.visible = preference !== 'sesame';
      breadGroups[1]!.visible = preference !== 'honey';
    },
    dispose() {
      groundMap.dispose();
      signMap.dispose();
      holoMap.dispose();
      hologram.material.dispose();
      runeMaterial.dispose();
      violetMaterial.dispose();
      materials.forEach((value) => value.dispose());
      groundMat.dispose();
    },
  };
}
