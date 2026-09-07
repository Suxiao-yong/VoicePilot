import * as THREE from "three";

/**
 * 谛听桌宠场景(桌宠化改造)。
 *
 * 从 D:/voicepilot/diting-desktop-pet.html 的 Three.js 程序化建模移植:
 * 山海经神兽谛听 —— 玉翡身体 #3AA090 + 云鬃鎏金尖 + 朱璎项圈,杏仁利眼 +
 * 金眼线,4 组动画(idle 英气站姿 / proud 挺胸 / wind 御风 / shake 抖鬃)+
 * 随机眨眼系统。纯代码生成,无外部模型文件。
 *
 * 相对 demo 的改造:
 * - scene.background = null → 配合 pet 窗口 transparent,只有兽身可见
 * - 去掉 OrbitControls / GLTFExporter / 点头彩蛋 / demo 页 UI
 * - 阴影贴图 2048→1024、保留小片 ShadowMaterial 地面投影(接地感)
 * - setLevel(rms):录音时驱动鬃毛摆幅,御风随音量起舞
 */

export type DitingAnim = "idle" | "proud" | "wind" | "shake";

export interface DitingHandle {
  setAnim(anim: DitingAnim): void;
  /** 归一化 RMS 电平 0..=1(audio-level 事件),录音态驱动鬃毛摆幅 */
  setLevel(level: number): void;
  dispose(): void;
}

export function createDiting(canvas: HTMLCanvasElement): DitingHandle {
  const renderer = new THREE.WebGLRenderer({
    canvas,
    antialias: true,
    alpha: true,
  });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.setClearColor(0x000000, 0);
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.06;
  renderer.outputColorSpace = THREE.SRGBColorSpace;

  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(34, 1, 0.1, 100);
  camera.position.set(0, 0.78, 2.72);

  // ===== 灯光(demo 同款:PBR 主光 + 补光 + 鎏金轮辉) =====
  scene.add(new THREE.HemisphereLight(0xffffff, 0xffeedd, 0.9));
  const sun = new THREE.DirectionalLight(0xfff6e8, 1.22);
  sun.position.set(2.4, 3.4, 2.0);
  sun.castShadow = true;
  sun.shadow.mapSize.set(1024, 1024);
  sun.shadow.camera.near = 0.1;
  sun.shadow.camera.far = 8;
  sun.shadow.camera.left = -2.5;
  sun.shadow.camera.right = 2.5;
  sun.shadow.camera.top = 2.5;
  sun.shadow.camera.bottom = -2.5;
  sun.shadow.bias = -0.0006;
  scene.add(sun);
  const fill = new THREE.DirectionalLight(0xcdeee2, 0.52);
  fill.position.set(-2.0, 1.4, -1.6);
  scene.add(fill);
  const rim = new THREE.PointLight(0xc9a86a, 7, 5);
  rim.position.set(1.8, 1.7, -1.2);
  scene.add(rim);
  const keyGold = new THREE.DirectionalLight(0xffe8a0, 0.45);
  keyGold.position.set(1.2, 2.0, 1.5);
  scene.add(keyGold);

  // 接地投影(透明窗口里只留一片柔影)
  const shadowMat = new THREE.ShadowMaterial({ opacity: 0.13 });
  const shadowPlane = new THREE.Mesh(
    new THREE.CircleGeometry(0.95, 32),
    shadowMat,
  );
  shadowPlane.rotation.x = -Math.PI / 2;
  shadowPlane.position.y = 0.006;
  shadowPlane.receiveShadow = true;
  scene.add(shadowPlane);

  // ===== 材质(canvas 程序化贴图:d玉纹 + 云鬃) =====
  function makeJadeTex(): THREE.CanvasTexture {
    const c = document.createElement("canvas");
    c.width = 1024;
    c.height = 1024;
    const ctx = c.getContext("2d")!;
    ctx.fillStyle = "#2F9E8A";
    ctx.fillRect(0, 0, 1024, 1024);
    for (let i = 0; i < 14; i++) {
      const x = Math.random() * 1024,
        y = Math.random() * 1024,
        r = 90 + Math.random() * 150;
      const g = ctx.createRadialGradient(x, y, 0, x, y, r);
      g.addColorStop(0, "rgba(126,216,184,0.22)");
      g.addColorStop(0.5, "rgba(90,180,150,0.13)");
      g.addColorStop(1, "rgba(47,158,138,0)");
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.strokeStyle = "rgba(205,240,228,0.34)";
    ctx.lineWidth = 9;
    ctx.lineCap = "round";
    for (let y = 90; y < 1024; y += 108) {
      ctx.beginPath();
      for (let x = 0; x <= 1024; x += 8) {
        const yy =
          y + Math.sin(x * 0.011 + y * 0.009) * 26 + Math.cos(x * 0.007) * 12;
        if (x === 0) ctx.moveTo(x, yy);
        else ctx.lineTo(x, yy);
      }
      ctx.stroke();
      ctx.strokeStyle = "rgba(255,255,255,0.16)";
      ctx.lineWidth = 2.5;
      ctx.stroke();
      ctx.strokeStyle = "rgba(205,240,228,0.34)";
      ctx.lineWidth = 9;
    }
    for (let i = 0; i < 10; i++) {
      const x = 140 + Math.random() * 740,
        y = 180 + Math.random() * 640;
      ctx.strokeStyle = "rgba(232,250,242,0.26)";
      ctx.lineWidth = 5.5;
      ctx.beginPath();
      ctx.arc(x, y, 22 + Math.random() * 18, -0.2, Math.PI * 1.65);
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(x + 10, y + 8, 12, 0, Math.PI * 1.4);
      ctx.stroke();
    }
    const tex = new THREE.CanvasTexture(c);
    tex.wrapS = tex.wrapT = THREE.RepeatWrapping;
    tex.colorSpace = THREE.SRGBColorSpace;
    tex.anisotropy = 8;
    return tex;
  }

  function makeManeTex(): THREE.CanvasTexture {
    const c = document.createElement("canvas");
    c.width = 512;
    c.height = 512;
    const ctx = c.getContext("2d")!;
    ctx.fillStyle = "#FFFEF8";
    ctx.fillRect(0, 0, 512, 512);
    for (let i = 0; i < 220; i++) {
      const x = Math.random() * 512,
        y = Math.random() * 512,
        r = 18 + Math.random() * 26;
      const g = ctx.createRadialGradient(x, y, 0, x, y, r);
      g.addColorStop(0, "rgba(255,250,232,0.22)");
      g.addColorStop(1, "rgba(255,254,248,0)");
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
    }
    for (let i = 0; i < 120; i++) {
      const x = Math.random() * 512,
        y = Math.random() * 512;
      ctx.fillStyle = `rgba(201,168,106,${0.04 + Math.random() * 0.06})`;
      ctx.fillRect(x, y, 1.2, 1.2);
    }
    const tex = new THREE.CanvasTexture(c);
    tex.colorSpace = THREE.SRGBColorSpace;
    return tex;
  }

  const jadeTex = makeJadeTex(),
    maneTex = makeManeTex();

  const matJade = new THREE.MeshStandardMaterial({
    color: 0x3aa090,
    map: jadeTex,
    roughness: 0.66,
    metalness: 0.02,
  });
  const matJadeDark = new THREE.MeshStandardMaterial({
    color: 0x2a7a68,
    map: jadeTex,
    roughness: 0.7,
  });
  const matJadeLight = new THREE.MeshStandardMaterial({
    color: 0x7ed8b8,
    roughness: 0.82,
  });
  const matJadePale = new THREE.MeshStandardMaterial({
    color: 0xc5ede2,
    roughness: 0.9,
  });
  const matGold = new THREE.MeshStandardMaterial({
    color: 0xc9a86a,
    roughness: 0.34,
    metalness: 0.62,
  });
  const matCream = new THREE.MeshStandardMaterial({
    color: 0xfffdf0,
    map: maneTex,
    roughness: 0.98,
  });
  const matCloud = new THREE.MeshStandardMaterial({
    color: 0xfffef8,
    map: maneTex,
    roughness: 0.98,
  });
  const matCloudShadow = new THREE.MeshStandardMaterial({
    color: 0xe8ead8,
    roughness: 1.0,
  });
  const matRed = new THREE.MeshStandardMaterial({
    color: 0xd93a2b,
    roughness: 0.6,
  });
  const matRedDark = new THREE.MeshStandardMaterial({
    color: 0xa93226,
    roughness: 0.68,
  });
  const matRedBright = new THREE.MeshStandardMaterial({
    color: 0xe84a3a,
    roughness: 0.52,
  });
  const matJadeBead = new THREE.MeshPhysicalMaterial({
    color: 0x4ec8a8,
    roughness: 0.22,
    clearcoat: 0.65,
    clearcoatRoughness: 0.22,
    transmission: 0.08,
    thickness: 0.04,
  });
  const matJadeBeadLarge = new THREE.MeshPhysicalMaterial({
    color: 0x3dd1b0,
    roughness: 0.18,
    clearcoat: 0.85,
    clearcoatRoughness: 0.18,
    transmission: 0.12,
    thickness: 0.05,
  });
  const matBeige = new THREE.MeshStandardMaterial({
    color: 0xd8b89e,
    roughness: 0.84,
  });
  const matDarkPaw = new THREE.MeshStandardMaterial({
    color: 0x1a3d3d,
    roughness: 0.86,
    map: jadeTex,
  });
  const matEyeBlack = new THREE.MeshStandardMaterial({
    color: 0x0a0a0a,
    roughness: 0.22,
  });
  const matWhite = new THREE.MeshStandardMaterial({
    color: 0xfffef8,
    roughness: 0.84,
  });
  const matMouthRed = new THREE.MeshStandardMaterial({
    color: 0x8b1e1e,
    roughness: 0.8,
  });
  const matGum = new THREE.MeshStandardMaterial({
    color: 0xd95a4a,
    roughness: 0.72,
  });

  // ===== 建模(demo 同款几何,挺拔英气版骨相) =====
  const diting = new THREE.Group();
  scene.add(diting);
  const bodyGroup = new THREE.Group();
  diting.add(bodyGroup);
  bodyGroup.position.y = 0.32;

  const body = new THREE.Mesh(new THREE.SphereGeometry(0.46, 28, 22), matJade);
  body.scale.set(1.06, 0.94, 0.98);
  body.position.set(0, 0.18, 0);
  body.castShadow = true;
  bodyGroup.add(body);
  const chestMuscle = new THREE.Mesh(
    new THREE.SphereGeometry(0.18, 14, 10),
    matJade,
  );
  chestMuscle.scale.set(1.55, 0.85, 0.55);
  chestMuscle.position.set(0, 0.12, 0.3);
  bodyGroup.add(chestMuscle);
  const belly = new THREE.Mesh(
    new THREE.SphereGeometry(0.26, 20, 14),
    matJadePale,
  );
  belly.scale.set(0.92, 1.0, 0.52);
  belly.position.set(0, -0.08, 0.32);
  bodyGroup.add(belly);
  for (let i = 0; i < 3; i++) {
    const wave = new THREE.Mesh(
      new THREE.TorusGeometry(0.2 + i * 0.042, 0.011, 6, 18, Math.PI),
      new THREE.MeshStandardMaterial({
        color: 0xa8e0d2,
        transparent: true,
        opacity: 0.36,
      }),
    );
    wave.position.set(0, -0.02 - i * 0.04, 0.36 + i * 0.008);
    wave.rotation.x = 0.55;
    wave.rotation.z = Math.PI;
    bodyGroup.add(wave);
  }

  function frontLeg(x: number): THREE.Group {
    const g = new THREE.Group();
    g.position.set(x, -0.04, 0.28);
    const upper = new THREE.Mesh(
      new THREE.CapsuleGeometry(0.105, 0.22, 8, 12),
      matJade,
    );
    upper.position.y = 0.11;
    upper.castShadow = true;
    g.add(upper);
    const w = new THREE.Mesh(
      new THREE.TorusGeometry(0.072, 0.008, 6, 12),
      matJadeLight,
    );
    w.rotation.x = Math.PI / 2;
    w.position.set(0, 0.09, 0.06);
    w.scale.set(1, 1, 0.6);
    g.add(w);
    const goldRing = new THREE.Mesh(
      new THREE.TorusGeometry(0.068, 0.004, 6, 12),
      matGold,
    );
    goldRing.rotation.x = Math.PI / 2;
    goldRing.position.set(0, 0.02, 0.065);
    g.add(goldRing);
    const lower = new THREE.Mesh(
      new THREE.CapsuleGeometry(0.092, 0.11, 8, 10),
      matJade,
    );
    lower.position.set(0, -0.08, 0.02);
    g.add(lower);
    const paw = new THREE.Mesh(
      new THREE.SphereGeometry(0.102, 14, 10),
      matDarkPaw,
    );
    paw.scale.set(1.12, 0.7, 1.04);
    paw.position.set(0, -0.18, 0.04);
    paw.castShadow = true;
    g.add(paw);
    for (let i = -1; i <= 1; i++) {
      const nail = new THREE.Mesh(
        new THREE.CapsuleGeometry(0.015, 0.022, 4, 6),
        matRedBright,
      );
      nail.rotation.x = Math.PI / 2;
      nail.position.set(i * 0.03, -0.18, 0.105);
      g.add(nail);
      const tip = new THREE.Mesh(
        new THREE.SphereGeometry(0.008, 5, 5),
        matGold,
      );
      tip.position.set(0, 0.014, 0);
      nail.add(tip);
    }
    return g;
  }
  bodyGroup.add(frontLeg(-0.19));
  bodyGroup.add(frontLeg(0.19));

  function hindLeg(x: number): THREE.Group {
    const g = new THREE.Group();
    g.position.set(x, -0.02, -0.1);
    const thigh = new THREE.Mesh(
      new THREE.SphereGeometry(0.19, 16, 12),
      matJade,
    );
    thigh.scale.set(0.92, 0.8, 1.0);
    thigh.castShadow = true;
    g.add(thigh);
    const swirl = new THREE.Mesh(
      new THREE.TorusGeometry(0.086, 0.01, 6, 14),
      matJadeLight,
    );
    swirl.position.set(0, 0.04, 0.11);
    swirl.rotation.x = 0.6;
    swirl.scale.set(1, 0.7, 1);
    g.add(swirl);
    const goldSwirl = new THREE.Mesh(
      new THREE.TorusGeometry(0.082, 0.004, 6, 14),
      matGold,
    );
    goldSwirl.position.set(0, 0.04, 0.115);
    goldSwirl.rotation.x = 0.6;
    goldSwirl.scale.set(1, 0.7, 1);
    g.add(goldSwirl);
    const shin = new THREE.Mesh(
      new THREE.SphereGeometry(0.135, 14, 10),
      matJade,
    );
    shin.scale.set(1, 0.7, 1.02);
    shin.position.set(0, -0.12, 0.12);
    g.add(shin);
    const paw = new THREE.Mesh(
      new THREE.SphereGeometry(0.106, 14, 10),
      matDarkPaw,
    );
    paw.scale.set(1.18, 0.66, 1.08);
    paw.position.set(0, -0.2, 0.16);
    g.add(paw);
    for (let i = -1; i <= 1; i++) {
      const nail = new THREE.Mesh(
        new THREE.CapsuleGeometry(0.014, 0.02, 4, 6),
        matRedBright,
      );
      nail.rotation.x = Math.PI / 2;
      nail.position.set(i * 0.028, -0.19, 0.215);
      g.add(nail);
    }
    const anklet = new THREE.Mesh(
      new THREE.TorusGeometry(0.076, 0.007, 6, 12),
      matGold,
    );
    anklet.rotation.x = Math.PI / 2;
    anklet.position.set(0, -0.08, 0.14);
    g.add(anklet);
    return g;
  }
  bodyGroup.add(hindLeg(-0.29));
  bodyGroup.add(hindLeg(0.29));

  const tail = new THREE.Mesh(new THREE.SphereGeometry(0.11, 12, 10), matCloud);
  tail.scale.set(1, 0.62, 1.18);
  tail.position.set(0, 0.02, -0.41);
  tail.castShadow = true;
  bodyGroup.add(tail);

  // 云鬃(御风飘带 + 金尖)
  const maneGroup = new THREE.Group();
  bodyGroup.add(maneGroup);
  maneGroup.position.set(0, 0.6, 0.1);
  const cloudPositions: [number, number, number, number][] = [
    [0, 0.36, -0.1, 0.34],
    [-0.23, 0.3, 0.0, 0.27],
    [0.23, 0.3, 0.0, 0.27],
    [-0.36, 0.16, 0.06, 0.25],
    [0.36, 0.16, 0.06, 0.25],
    [-0.29, 0.04, 0.14, 0.21],
    [0.29, 0.04, 0.14, 0.21],
    [0, 0.12, -0.32, 0.29],
    [-0.17, 0.08, -0.26, 0.23],
    [0.17, 0.08, -0.26, 0.23],
    [-0.11, 0.28, -0.18, 0.18],
    [0.11, 0.28, -0.18, 0.18],
  ];
  cloudPositions.forEach(([x, y, z, r], i) => {
    const puff = new THREE.Mesh(
      new THREE.SphereGeometry(r, 16, 12),
      i % 2 === 0 ? matCloud : matCream,
    );
    puff.position.set(x, y, z);
    puff.scale.set(1, 0.78 + Math.random() * 0.1, 0.86);
    puff.castShadow = true;
    puff.receiveShadow = true;
    puff.rotation.y = Math.random() * 0.3;
    puff.rotation.z = (Math.random() - 0.5) * 0.18;
    if (i < 3) puff.scale.y *= 1.12;
    maneGroup.add(puff);
    if (i < 6) {
      const curl = new THREE.Mesh(
        new THREE.TorusGeometry(r * 0.36, 0.02, 6, 14, Math.PI * 1.3),
        matCloud,
      );
      curl.position.set(x * 0.92, y + r * 0.3, z + 0.05);
      curl.rotation.x = 0.62;
      curl.rotation.z = (Math.random() - 0.5) * 0.5;
      maneGroup.add(curl);
      const goldTip = new THREE.Mesh(
        new THREE.TorusGeometry(r * 0.22, 0.005, 6, 10, Math.PI * 1.1),
        matGold,
      );
      goldTip.position.set(x * 0.95, y + r * 0.38, z + 0.06);
      goldTip.rotation.x = 0.62;
      goldTip.rotation.z = (Math.random() - 0.5) * 0.5;
      maneGroup.add(goldTip);
    }
  });
  for (let i = 0; i < 6; i++) {
    const s = new THREE.Mesh(
      new THREE.SphereGeometry(0.085 + Math.random() * 0.05, 10, 8),
      matCloudShadow,
    );
    s.position.set(
      (Math.random() - 0.5) * 0.68,
      0.32 + Math.random() * 0.16,
      -0.18 + (Math.random() - 0.5) * 0.12,
    );
    s.scale.set(1, 0.52, 0.68);
    s.material.transparent = true;
    s.material.opacity = 0.38;
    maneGroup.add(s);
  }

  // 头(窄脸 + 金边锋颌)
  const headGroup = new THREE.Group();
  headGroup.position.set(0, 0.64, 0.24);
  bodyGroup.add(headGroup);
  const head = new THREE.Mesh(new THREE.SphereGeometry(0.33, 26, 20), matJade);
  head.scale.set(1.04, 1.06, 0.98);
  head.castShadow = true;
  headGroup.add(head);
  const jawLine = new THREE.Mesh(
    new THREE.CapsuleGeometry(0.018, 0.22, 4, 8),
    matGold,
  );
  jawLine.rotation.z = Math.PI / 2;
  jawLine.position.set(0, -0.18, 0.18);
  jawLine.scale.set(1, 1, 0.5);
  headGroup.add(jawLine);
  for (const side of [-1, 1]) {
    const jawSide = new THREE.Mesh(
      new THREE.CapsuleGeometry(0.012, 0.14, 4, 6),
      matGold,
    );
    jawSide.position.set(side * 0.14, -0.12, 0.16);
    jawSide.rotation.z = side * 0.45;
    jawSide.rotation.x = 0.25;
    headGroup.add(jawSide);
    const curl = new THREE.Mesh(
      new THREE.TorusGeometry(0.078, 0.016, 8, 16, Math.PI * 1.6),
      matJadeLight,
    );
    curl.position.set(side * 0.2, -0.05, 0.26);
    curl.rotation.z = side * 0.85;
    curl.rotation.x = 0.35;
    headGroup.add(curl);
    const inner = new THREE.Mesh(
      new THREE.TorusGeometry(0.042, 0.01, 6, 12, Math.PI * 1.4),
      new THREE.MeshStandardMaterial({
        color: 0xfffef8,
        transparent: true,
        opacity: 0.5,
      }),
    );
    inner.position.set(side * 0.2, -0.05, 0.275);
    inner.rotation.z = side * 0.85;
    inner.rotation.x = 0.35;
    headGroup.add(inner);
  }
  const chinWhite = new THREE.Mesh(
    new THREE.SphereGeometry(0.16, 16, 12),
    matWhite,
  );
  chinWhite.scale.set(1.28, 0.68, 0.82);
  chinWhite.position.set(0, -0.13, 0.3);
  headGroup.add(chinWhite);
  const cheekWhiteL = new THREE.Mesh(
    new THREE.SphereGeometry(0.1, 12, 8),
    matWhite,
  );
  cheekWhiteL.scale.set(1, 0.82, 0.58);
  cheekWhiteL.position.set(-0.18, -0.01, 0.28);
  headGroup.add(cheekWhiteL);
  const cheekWhiteR = cheekWhiteL.clone();
  cheekWhiteR.position.x = 0.18;
  headGroup.add(cheekWhiteR);
  const nose = new THREE.Mesh(new THREE.SphereGeometry(0.036, 10, 8), matBeige);
  nose.scale.set(1.32, 0.82, 1.05);
  nose.position.set(0, 0.035, 0.38);
  headGroup.add(nose);
  const noseBridge = new THREE.Mesh(
    new THREE.CapsuleGeometry(0.012, 0.07, 4, 6),
    matBeige,
  );
  noseBridge.position.set(0, 0.08, 0.34);
  noseBridge.rotation.x = 0.18;
  headGroup.add(noseBridge);
  const nostrilL = new THREE.Mesh(
    new THREE.SphereGeometry(0.009, 6, 6),
    new THREE.MeshStandardMaterial({ color: 0x4a2e18 }),
  );
  nostrilL.scale.set(1, 1.3, 0.5);
  nostrilL.position.set(-0.014, 0.03, 0.39);
  headGroup.add(nostrilL);
  const nostrilR = nostrilL.clone();
  nostrilR.position.x = 0.014;
  headGroup.add(nostrilR);

  // 杏仁利眼 + 金眼线 + 可眨眼皮
  function makeEye(x: number): THREE.Group {
    const g = new THREE.Group();
    g.position.set(x, 0.065, 0.31);
    const sclera = new THREE.Mesh(
      new THREE.SphereGeometry(0.062, 16, 12),
      new THREE.MeshStandardMaterial({ color: 0xfffef8 }),
    );
    sclera.scale.set(1.14, 0.88, 0.34);
    sclera.rotation.z = x > 0 ? 0.08 : -0.08;
    g.add(sclera);
    const iris = new THREE.Mesh(
      new THREE.SphereGeometry(0.044, 16, 12),
      matEyeBlack,
    );
    iris.position.set(0, 0, 0.04);
    iris.scale.set(1.06, 1.06, 0.34);
    g.add(iris);
    const hi = new THREE.Mesh(
      new THREE.SphereGeometry(0.012, 8, 8),
      new THREE.MeshBasicMaterial({ color: 0xffffff }),
    );
    hi.position.set(0.016, 0.016, 0.054);
    g.add(hi);
    const hi2 = new THREE.Mesh(
      new THREE.SphereGeometry(0.005, 6, 6),
      new THREE.MeshBasicMaterial({
        color: 0xffffff,
        transparent: true,
        opacity: 0.82,
      }),
    );
    hi2.position.set(-0.009, -0.009, 0.05);
    g.add(hi2);
    const liner = new THREE.Mesh(
      new THREE.TorusGeometry(0.058, 0.004, 6, 16, Math.PI * 0.95),
      matGold,
    );
    liner.position.set(0, 0, 0.042);
    liner.rotation.x = 0.18;
    liner.rotation.z = x > 0 ? -0.12 : 0.12;
    g.add(liner);
    const lid = new THREE.Mesh(
      new THREE.SphereGeometry(0.066, 12, 8, 0, Math.PI * 2, 0, Math.PI * 0.42),
      matJadeDark,
    );
    lid.rotation.x = Math.PI;
    lid.position.set(0, 0.02, 0.012);
    lid.scale.set(1.08, 0.62, 0.5);
    lid.name = "eyelid";
    lid.visible = false;
    g.add(lid);
    return g;
  }
  const eyeL = makeEye(-0.128),
    eyeR = makeEye(0.128);
  headGroup.add(eyeL);
  headGroup.add(eyeR);
  for (const x of [-0.128, 0.128]) {
    const hl = new THREE.Mesh(
      new THREE.SphereGeometry(0.048, 8, 6),
      new THREE.MeshBasicMaterial({
        color: 0x7ed8b8,
        transparent: true,
        opacity: 0.14,
      }),
    );
    hl.scale.set(1.5, 0.42, 0.5);
    hl.position.set(x, 0.125, 0.3);
    headGroup.add(hl);
  }

  // 口部(獠牙)
  const mouthGroup = new THREE.Group();
  mouthGroup.position.set(0, -0.07, 0.36);
  headGroup.add(mouthGroup);
  const mouthInterior = new THREE.Mesh(
    new THREE.SphereGeometry(0.068, 12, 8, 0, Math.PI * 2, 0, Math.PI * 0.48),
    matMouthRed,
  );
  mouthInterior.rotation.x = Math.PI;
  mouthInterior.scale.set(1.28, 0.68, 0.52);
  mouthInterior.position.set(0, -0.018, 0);
  mouthGroup.add(mouthInterior);
  const gumUpper = new THREE.Mesh(
    new THREE.CapsuleGeometry(0.011, 0.13, 4, 8),
    matGum,
  );
  gumUpper.rotation.z = Math.PI / 2;
  gumUpper.position.set(0, 0.018, 0.02);
  mouthGroup.add(gumUpper);
  const gumLower = new THREE.Mesh(
    new THREE.CapsuleGeometry(0.009, 0.11, 4, 8),
    matGum,
  );
  gumLower.rotation.z = Math.PI / 2;
  gumLower.position.set(0, -0.052, 0.02);
  mouthGroup.add(gumLower);
  const fangUL = new THREE.Mesh(
    new THREE.ConeGeometry(0.02, 0.052, 6),
    matWhite,
  );
  fangUL.position.set(-0.062, 0.014, 0.03);
  fangUL.rotation.x = Math.PI - 0.22;
  fangUL.rotation.z = 0.18;
  mouthGroup.add(fangUL);
  const fangUR = fangUL.clone();
  fangUR.position.x = 0.062;
  fangUR.rotation.z = -0.18;
  mouthGroup.add(fangUR);
  const fangLL = new THREE.Mesh(
    new THREE.ConeGeometry(0.014, 0.036, 6),
    matWhite,
  );
  fangLL.position.set(-0.046, -0.042, 0.03);
  fangLL.rotation.x = 0.16;
  fangLL.rotation.z = -0.1;
  mouthGroup.add(fangLL);
  const fangLR = fangLL.clone();
  fangLR.position.x = 0.046;
  fangLR.rotation.z = 0.1;
  mouthGroup.add(fangLR);
  for (const x of [-0.026, 0.026]) {
    const t = new THREE.Mesh(new THREE.ConeGeometry(0.007, 0.018, 5), matWhite);
    t.position.set(x, -0.024, 0.04);
    t.rotation.x = Math.PI - 0.14;
    mouthGroup.add(t);
  }

  // 额顶翠冠 + 朱红螺旋
  const crestGroup = new THREE.Group();
  crestGroup.position.set(0, 0.3, 0.18);
  headGroup.add(crestGroup);
  const crestBase = new THREE.Mesh(
    new THREE.SphereGeometry(0.13, 16, 12),
    matJade,
  );
  crestBase.scale.set(1.42, 0.7, 0.52);
  crestBase.position.set(0, -0.055, 0);
  crestGroup.add(crestBase);
  for (const s of [-1, 1]) {
    const swirl = new THREE.Mesh(
      new THREE.TorusGeometry(0.06, 0.015, 8, 14, Math.PI * 1.5),
      matJadeDark,
    );
    swirl.position.set(s * 0.105, -0.035, 0.04);
    swirl.rotation.z = s * 0.75;
    swirl.rotation.x = 0.25;
    crestGroup.add(swirl);
    const inner = new THREE.Mesh(
      new THREE.TorusGeometry(0.03, 0.009, 6, 10, Math.PI * 1.3),
      matJadeLight,
    );
    inner.position.set(s * 0.105, -0.035, 0.055);
    inner.rotation.z = s * 0.75;
    inner.rotation.x = 0.25;
    crestGroup.add(inner);
  }
  const crestTop = new THREE.Mesh(
    new THREE.ConeGeometry(0.062, 0.15, 4),
    matJade,
  );
  crestTop.position.set(0, 0.13, 0);
  crestTop.scale.set(1, 1, 0.42);
  crestGroup.add(crestTop);
  const crestTopLight = new THREE.Mesh(
    new THREE.ConeGeometry(0.03, 0.068, 4),
    matGold,
  );
  crestTopLight.position.set(0, 0.16, 0.02);
  crestTopLight.scale.set(1, 1, 0.5);
  crestGroup.add(crestTopLight);
  const spiralGroup = new THREE.Group();
  spiralGroup.position.set(0, 0.025, 0.08);
  crestGroup.add(spiralGroup);
  const spiralBase = new THREE.Mesh(
    new THREE.CylinderGeometry(0.053, 0.053, 0.018, 16),
    matRed,
  );
  spiralBase.rotation.x = Math.PI / 2;
  spiralGroup.add(spiralBase);
  const spiralGoldRing = new THREE.Mesh(
    new THREE.RingGeometry(0.048, 0.054, 16),
    new THREE.MeshBasicMaterial({ color: 0xc9a86a, side: THREE.DoubleSide }),
  );
  spiralGoldRing.position.set(0, 0, 0.011);
  spiralGroup.add(spiralGoldRing);
  const spiralOuter = new THREE.Mesh(
    new THREE.RingGeometry(0.027, 0.046, 16),
    new THREE.MeshBasicMaterial({ color: 0xff8a6e, side: THREE.DoubleSide }),
  );
  spiralOuter.position.set(0, 0, 0.012);
  spiralGroup.add(spiralOuter);
  const spiralMid = new THREE.Mesh(
    new THREE.RingGeometry(0.015, 0.025, 16),
    new THREE.MeshBasicMaterial({ color: 0xfff2e2, side: THREE.DoubleSide }),
  );
  spiralMid.position.set(0, 0, 0.013);
  spiralGroup.add(spiralMid);
  const spiralCore = new THREE.Mesh(
    new THREE.CircleGeometry(0.013, 16),
    new THREE.MeshBasicMaterial({ color: 0xe84a3a, side: THREE.DoubleSide }),
  );
  spiralCore.position.set(0, 0, 0.014);
  spiralGroup.add(spiralCore);
  const spiralLine = new THREE.Mesh(
    new THREE.TorusGeometry(0.019, 0.004, 6, 16, Math.PI * 1.4),
    new THREE.MeshBasicMaterial({ color: 0xc0392b, side: THREE.DoubleSide }),
  );
  spiralLine.position.set(0, 0, 0.0135);
  spiralLine.rotation.z = 0.6;
  spiralGroup.add(spiralLine);
  const foreheadDot = new THREE.Mesh(
    new THREE.SphereGeometry(0.021, 10, 8),
    matRed,
  );
  foreheadDot.position.set(0, -0.105, 0.09);
  crestGroup.add(foreheadDot);
  const foreheadHi = new THREE.Mesh(
    new THREE.SphereGeometry(0.006, 6, 6),
    new THREE.MeshBasicMaterial({
      color: 0xffffff,
      transparent: true,
      opacity: 0.82,
    }),
  );
  foreheadHi.position.set(0.006, -0.1, 0.105);
  crestGroup.add(foreheadHi);
  const crestRim = new THREE.Mesh(
    new THREE.TorusGeometry(0.105, 0.006, 6, 16, Math.PI),
    new THREE.MeshStandardMaterial({
      color: 0xc9a86a,
      transparent: true,
      opacity: 0.62,
    }),
  );
  crestRim.position.set(0, -0.055, 0.06);
  crestRim.rotation.x = 0.32;
  crestGroup.add(crestRim);

  // 耳(金边)
  function ear(side: number): THREE.Group {
    const g = new THREE.Group();
    g.position.set(side * 0.19, 0.15, 0.06);
    const outer = new THREE.Mesh(
      new THREE.ConeGeometry(0.068, 0.15, 4),
      matJadeDark,
    );
    outer.rotation.z = side * -0.16;
    outer.position.set(side * 0.018, 0.055, 0);
    outer.scale.set(1, 1, 0.38);
    g.add(outer);
    const inner = new THREE.Mesh(
      new THREE.ConeGeometry(0.038, 0.085, 4),
      new THREE.MeshStandardMaterial({ color: 0x6b8a7f }),
    );
    inner.position.set(side * 0.009, 0.055, 0.028);
    inner.rotation.z = side * -0.16;
    inner.scale.set(1, 1, 0.5);
    g.add(inner);
    const goldRim = new THREE.Mesh(
      new THREE.TorusGeometry(0.032, 0.003, 6, 10, Math.PI),
      matGold,
    );
    goldRim.position.set(side * 0.009, 0.055, 0.032);
    goldRim.rotation.x = 0.35;
    goldRim.rotation.z = side * 0.2;
    g.add(goldRim);
    return g;
  }
  headGroup.add(ear(-1));
  headGroup.add(ear(1));

  // 朱璎项圈 + 玉珠 + 流苏
  const collarGroup = new THREE.Group();
  collarGroup.position.set(0, 0.19, 0.08);
  bodyGroup.add(collarGroup);
  const collar = new THREE.Mesh(
    new THREE.TorusGeometry(0.26, 0.021, 8, 20),
    matRed,
  );
  collar.rotation.x = Math.PI / 2 + 0.14;
  collar.position.set(0, -0.02, 0);
  collar.castShadow = true;
  collarGroup.add(collar);
  const collarGold = new THREE.Mesh(
    new THREE.TorusGeometry(0.262, 0.004, 8, 20),
    matGold,
  );
  collarGold.rotation.x = Math.PI / 2 + 0.14;
  collarGold.position.set(0, -0.02, 0.006);
  collarGroup.add(collarGold);
  const beadPositions: [number, number, number][] = [
    [-0.15, -0.06, 0.175],
    [-0.077, -0.1, 0.215],
    [0.077, -0.1, 0.215],
    [0.15, -0.06, 0.175],
  ];
  beadPositions.forEach(([x, y, z]) => {
    const bead = new THREE.Mesh(
      new THREE.SphereGeometry(0.031, 10, 8),
      matJadeBead,
    );
    bead.position.set(x, y, z);
    collarGroup.add(bead);
    const beadHi = new THREE.Mesh(
      new THREE.SphereGeometry(0.008, 6, 6),
      new THREE.MeshBasicMaterial({
        color: 0xffffff,
        transparent: true,
        opacity: 0.7,
      }),
    );
    beadHi.position.set(x + 0.007, y + 0.007, z + 0.017);
    collarGroup.add(beadHi);
    const redBase = new THREE.Mesh(
      new THREE.CylinderGeometry(0.021, 0.021, 0.008, 10),
      matRed,
    );
    redBase.position.set(x, y - 0.021, z);
    redBase.rotation.x = Math.PI / 2;
    collarGroup.add(redBase);
  });
  const bigBead = new THREE.Mesh(
    new THREE.SphereGeometry(0.07, 16, 12),
    matJadeBeadLarge,
  );
  bigBead.position.set(0, -0.155, 0.235);
  bigBead.castShadow = true;
  collarGroup.add(bigBead);
  const bigHi = new THREE.Mesh(
    new THREE.SphereGeometry(0.017, 8, 8),
    new THREE.MeshBasicMaterial({
      color: 0xffffff,
      transparent: true,
      opacity: 0.8,
    }),
  );
  bigHi.position.set(0.016, -0.14, 0.265);
  collarGroup.add(bigHi);
  const bigRedBase = new THREE.Mesh(
    new THREE.CylinderGeometry(0.036, 0.036, 0.01, 12),
    matRed,
  );
  bigRedBase.position.set(0, -0.195, 0.235);
  bigRedBase.rotation.x = Math.PI / 2;
  collarGroup.add(bigRedBase);
  const bigGoldRing = new THREE.Mesh(
    new THREE.TorusGeometry(0.042, 0.004, 8, 14),
    matGold,
  );
  bigGoldRing.rotation.x = Math.PI / 2;
  bigGoldRing.position.set(0, -0.155, 0.245);
  collarGroup.add(bigGoldRing);
  for (const x of [-0.115, 0, 0.115]) {
    const seg = new THREE.Mesh(
      new THREE.CapsuleGeometry(0.01, 0.038, 4, 6),
      matRed,
    );
    seg.rotation.z = Math.PI / 2;
    seg.position.set(x, -0.04, 0.175);
    collarGroup.add(seg);
  }
  const tassel = new THREE.Group();
  tassel.position.set(0, -0.215, 0.235);
  collarGroup.add(tassel);
  const tasselTop = new THREE.Mesh(
    new THREE.CylinderGeometry(0.017, 0.021, 0.026, 8),
    matRedDark,
  );
  tassel.add(tasselTop);
  const tasselGold = new THREE.Mesh(
    new THREE.CylinderGeometry(0.018, 0.018, 0.004, 12),
    matGold,
  );
  tasselGold.position.set(0, 0.012, 0);
  tassel.add(tasselGold);
  for (let i = 0; i < 7; i++) {
    const strand = new THREE.Mesh(
      new THREE.CylinderGeometry(0.003, 0.002, 0.105, 4),
      matRed,
    );
    strand.position.set((i - 3) * 0.0058, -0.068, 0);
    tassel.add(strand);
    const tip = new THREE.Mesh(new THREE.SphereGeometry(0.0045, 4, 4), matGold);
    tip.position.set((i - 3) * 0.0058, -0.12, 0);
    tassel.add(tip);
  }
  const tasselJewel = new THREE.Mesh(
    new THREE.SphereGeometry(0.013, 6, 6),
    matJadeBead,
  );
  tasselJewel.position.set(0, -0.038, 0.02);
  tassel.add(tasselJewel);
  for (const s of [-1, 1]) {
    const sideTassel = new THREE.Group();
    sideTassel.position.set(s * 0.19, -0.078, 0.155);
    collarGroup.add(sideTassel);
    const r = new THREE.Mesh(new THREE.BoxGeometry(0.02, 0.058, 0.006), matRed);
    sideTassel.add(r);
    const gld = new THREE.Mesh(
      new THREE.BoxGeometry(0.021, 0.003, 0.007),
      matGold,
    );
    gld.position.set(0, 0.012, 0.004);
    sideTassel.add(gld);
    const rb = new THREE.Mesh(
      new THREE.SphereGeometry(0.0065, 5, 5),
      matJadeBead,
    );
    rb.position.set(0, -0.033, 0.008);
    sideTassel.add(rb);
  }

  diting.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (mesh.isMesh) {
      mesh.castShadow = true;
      mesh.receiveShadow = true;
    }
  });
  bodyGroup.rotation.y = 0.04;

  // ===== 尺寸自适应 =====
  function onResize(): void {
    const rect = canvas.getBoundingClientRect();
    const w = rect.width || 1,
      h = rect.height || 1;
    renderer.setSize(w, h, false);
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
  }
  const ro = new ResizeObserver(onResize);
  ro.observe(canvas);
  onResize();

  // ===== 动画循环(呼吸 + 四态 + 眨眼 + 录音电平驱动) =====
  let currentAnim: DitingAnim = "idle";
  let level = 0;
  let disposed = false;

  let t = 0,
    blinkTimer = 0,
    nextBlink = 2.6 + Math.random() * 1.6;

  function frame(): void {
    if (disposed) return;
    requestAnimationFrame(frame);
    const dt = 0.016;
    t += dt;
    // 录音电平平滑衰减(无新事件时缓缓回落)
    level *= 0.92;
    const breath = Math.sin(t * 0.9) * 0.005;
    bodyGroup.position.y =
      0.32 + breath + (currentAnim === "wind" ? level * 0.015 : 0);
    headGroup.position.y = 0.64 + Math.sin(t * 0.9 + 0.3) * 0.0035;
    maneGroup.position.y = 0.6 + Math.sin(t * 0.7) * 0.003;
    tassel.rotation.z = Math.sin(t * 0.9) * 0.06;
    tassel.position.x = Math.sin(t * 0.7) * 0.003;

    if (currentAnim === "idle") {
      diting.rotation.y = 0.04 + Math.sin(t * 0.22) * 0.035;
      bodyGroup.rotation.y = 0.04 + Math.sin(t * 0.22) * 0.015;
      maneGroup.rotation.y = Math.sin(t * 0.28) * 0.04;
    } else if (currentAnim === "proud") {
      const b = Math.sin(t * 0.85);
      body.scale.set(1 + b * 0.008, 1 + b * 0.012, 1);
      chestMuscle.scale.set(1.55 + b * 0.04, 0.85, 0.55);
      headGroup.rotation.x = -0.04 + b * 0.015;
      headGroup.position.z = 0.24 + b * 0.006;
      diting.rotation.y = 0.04 + Math.sin(t * 0.22) * 0.02;
    } else if (currentAnim === "wind") {
      // 御风:鬃毛飘动幅度随录音电平放大(安静说话轻飘,大声说话狂舞)
      const amp = 0.22 * (0.35 + level * 1.65);
      const w = Math.sin(t * 1.8) * amp;
      maneGroup.rotation.y = w * 0.45;
      maneGroup.rotation.z = w * 0.12;
      maneGroup.children.forEach((c, i) => {
        const mesh = c as THREE.Mesh;
        if (mesh.geometry && mesh.geometry.type === "SphereGeometry") {
          mesh.position.x += Math.sin(t * 1.2 + i) * (0.0008 + level * 0.002);
          mesh.rotation.z = Math.sin(t * 1.4 + i * 0.7) * 0.08 * (0.5 + level);
        }
      });
      collarGroup.rotation.y = w * 0.08;
      tassel.rotation.z = w * 0.22;
      headGroup.rotation.y = w * 0.06;
    } else if (currentAnim === "shake") {
      const s = Math.sin(t * 6.5) * 0.08;
      maneGroup.rotation.z = s * 0.55;
      maneGroup.rotation.x = Math.abs(s) * 0.18;
      headGroup.rotation.z = s * 0.2;
      collarGroup.rotation.z = s * 0.14;
      diting.rotation.y = 0.04 + s * 0.1;
    }

    // 眨眼系统(全程保留)
    blinkTimer += dt;
    const lidL = eyeL.getObjectByName("eyelid");
    const lidR = eyeR.getObjectByName("eyelid");
    if (blinkTimer > nextBlink && lidL && lidR) {
      const ph = (blinkTimer - nextBlink) / 0.11;
      if (ph < 1) {
        const s = ph < 0.5 ? ph * 2 : (1 - ph) * 2;
        const sy = 1 - s * 0.94;
        lidL.visible = true;
        lidR.visible = true;
        lidL.scale.y = sy + 0.06;
        lidR.scale.y = sy + 0.06;
      } else if (ph < 1.16) {
        lidL.visible = false;
        lidR.visible = false;
      } else {
        blinkTimer = 0;
        nextBlink = 2.4 + Math.random() * 2.8;
        lidL.visible = false;
        lidR.visible = false;
      }
    } else if (lidL && lidR) {
      // 非眨眼期确保眼皮收起
      if (ph_not_blinking()) {
        lidL.visible = false;
        lidR.visible = false;
      }
    }

    renderer.render(scene, camera);
  }

  function ph_not_blinking(): boolean {
    const ph = (blinkTimer - nextBlink) / 0.11;
    return !(ph >= 0 && ph < 1);
  }

  frame();

  return {
    setAnim(anim: DitingAnim): void {
      currentAnim = anim;
    },
    setLevel(v: number): void {
      level = Math.min(1, Math.max(0, v));
    },
    dispose(): void {
      disposed = true;
      ro.disconnect();
      scene.traverse((o) => {
        const mesh = o as THREE.Mesh;
        if (mesh.isMesh) {
          mesh.geometry.dispose();
          const m = mesh.material as THREE.Material | THREE.Material[];
          if (Array.isArray(m)) m.forEach((mm) => mm.dispose());
          else m.dispose();
        }
      });
      jadeTex.dispose();
      maneTex.dispose();
      // 只 dispose 不 forceContextLoss:StrictMode 双挂载会在同一 canvas 上
      // 二次 new WebGLRenderer,强杀上下文后 getContext 返回 null,
      // 第二次初始化直接抛 "reading 'precision'"。dispose 已释放全部 GPU 资源。
      renderer.dispose();
    },
  };
}
