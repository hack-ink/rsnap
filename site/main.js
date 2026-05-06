import * as THREE from "https://cdn.jsdelivr.net/npm/three@0.165.0/build/three.module.js";

const canvas = document.querySelector("#capture-field");
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

document.documentElement.classList.add("motion-ready");

const renderer = new THREE.WebGLRenderer({
  canvas,
  antialias: true,
  alpha: true,
  powerPreference: "high-performance",
});

renderer.setClearColor(0x050506, 0);
renderer.outputColorSpace = THREE.SRGBColorSpace;
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.toneMappingExposure = 1.12;

const scene = new THREE.Scene();
scene.fog = new THREE.FogExp2(0x050506, 0.033);

const camera = new THREE.PerspectiveCamera(34, 1, 0.1, 100);
camera.position.set(0, 0.08, 7.6);

const pointer = {
  x: 0,
  y: 0,
  targetX: 0,
  targetY: 0,
};

const scrollState = {
  target: 0,
  value: 0,
};

const layoutState = {
  x: 0,
  y: 0,
  scale: 1,
};

const stage = new THREE.Group();
const captureRig = new THREE.Group();
const volumeRig = new THREE.Group();
const orbitRig = new THREE.Group();
stage.add(volumeRig, orbitRig, captureRig);
scene.add(stage);

const animatedMaterials = [];

function trackMaterial(material) {
  animatedMaterials.push(material);
  return material;
}

function makeSeededRandom(seed = 1) {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 4294967296;
  };
}

function makeGlowMaterial(color, opacity, radius = 0.006) {
  return new THREE.MeshBasicMaterial({
    color,
    transparent: true,
    opacity,
    blending: THREE.AdditiveBlending,
    depthWrite: false,
  });
}

function makeTube(points, radius, color, opacity, tubularSegments = 96) {
  const curve = new THREE.CatmullRomCurve3(points);
  const geometry = new THREE.TubeGeometry(curve, tubularSegments, radius, 10, false);
  return new THREE.Mesh(geometry, makeGlowMaterial(color, opacity, radius));
}

function makeTubeSegment(start, end, radius, color, opacity) {
  return makeTube([start, end], radius, color, opacity, 1);
}

function makeCaptureSurfaceGeometry(width = 2.82, height = 1.62, cols = 88, rows = 52) {
  const geometry = new THREE.PlaneGeometry(width, height, cols, rows);
  const positions = geometry.attributes.position;

  for (let index = 0; index < positions.count; index += 1) {
    const x = positions.getX(index);
    const y = positions.getY(index);
    const u = x / width + 0.5;
    const v = y / height + 0.5;
    const bow = Math.sin(u * Math.PI) * 0.16;
    const roll = (v - 0.5) * Math.sin(u * Math.PI * 1.35) * 0.08;
    const micro = Math.sin(u * 21.0 + v * 8.0) * 0.006;
    positions.setZ(index, bow + roll + micro);
  }

  positions.needsUpdate = true;
  geometry.computeVertexNormals();
  return geometry;
}

function makeCaptureSurfaceMaterial() {
  return trackMaterial(
    new THREE.ShaderMaterial({
      transparent: true,
      depthWrite: false,
      side: THREE.DoubleSide,
      uniforms: {
        uTime: { value: 0 },
      },
      vertexShader: `
        varying vec2 vUv;
        varying float vDepth;

        void main() {
          vUv = uv;
          vDepth = position.z;
          gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
        }
      `,
      fragmentShader: `
        precision highp float;

        varying vec2 vUv;
        varying float vDepth;
        uniform float uTime;

        float ridge(float value, float center, float width) {
          return exp(-pow((value - center) / width, 2.0));
        }

        float thinLine(float value, float count, float sharpness) {
          float dist = abs(fract(value * count) - 0.5) * 2.0;
          return pow(max(0.0, 1.0 - dist), sharpness);
        }

        void main() {
          vec2 p = vUv - 0.5;
          float edgeX = smoothstep(0.5, 0.36, abs(p.x));
          float edgeY = smoothstep(0.5, 0.34, abs(p.y));
          float body = edgeX * edgeY;
          float scan = ridge(vUv.x + sin(vUv.y * 2.2 + uTime * 0.26) * 0.025, 0.62, 0.055);
          float focus = ridge(length(p * vec2(1.25, 1.0)), 0.22, 0.34);
          float gridX = thinLine(vUv.x + vUv.y * 0.022, 18.0, 20.0);
          float gridY = thinLine(vUv.y + sin(vUv.x * 3.14159) * 0.012, 12.0, 22.0);
          float data = thinLine(vUv.x + vUv.y * 0.36 + uTime * 0.025, 9.0, 42.0);
          float rim = smoothstep(0.4, 0.5, abs(p.x)) + smoothstep(0.39, 0.5, abs(p.y));
          float grain = sin(vUv.x * 117.0 + vUv.y * 61.0 + uTime * 0.18) * 0.012;

          vec3 cyan = vec3(0.46, 0.96, 1.0);
          vec3 warm = vec3(1.0, 0.82, 0.52);
          vec3 base = vec3(0.012, 0.02, 0.022);
          vec3 color = base;
          color += cyan * (scan * 0.78 + focus * 0.14 + gridX * 0.04 + gridY * 0.035);
          color += warm * data * 0.04;
          color += cyan * clamp(vDepth, 0.0, 0.28) * 0.18;

          float alpha = 0.16 + scan * 0.36 + focus * 0.18 + gridX * 0.055 + gridY * 0.05 + data * 0.04 + rim * 0.11 + grain;
          alpha *= body;

          gl_FragColor = vec4(color, alpha);
        }
      `,
    }),
  );
}

function makeScanSheetMaterial() {
  return trackMaterial(
    new THREE.ShaderMaterial({
      transparent: true,
      depthWrite: false,
      side: THREE.DoubleSide,
      blending: THREE.AdditiveBlending,
      uniforms: {
        uTime: { value: 0 },
      },
      vertexShader: `
        varying vec2 vUv;

        void main() {
          vUv = uv;
          gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
        }
      `,
      fragmentShader: `
        precision highp float;

        varying vec2 vUv;
        uniform float uTime;

        void main() {
          vec2 p = vUv - 0.5;
          float spine = exp(-pow(p.x / 0.12, 2.0));
          float falloff = smoothstep(0.5, 0.08, abs(p.y));
          float pulse = 0.78 + sin(uTime * 1.4) * 0.12;
          vec3 color = vec3(0.36, 0.94, 1.0) * (0.38 + spine * 0.68);
          float alpha = spine * falloff * pulse * 0.32;
          gl_FragColor = vec4(color, alpha);
        }
      `,
    }),
  );
}

function makeDepthWashMaterial() {
  return trackMaterial(
    new THREE.ShaderMaterial({
      transparent: true,
      depthWrite: false,
      uniforms: {
        uTime: { value: 0 },
      },
      vertexShader: `
        varying vec2 vUv;

        void main() {
          vUv = uv;
          gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
        }
      `,
      fragmentShader: `
        precision highp float;

        varying vec2 vUv;
        uniform float uTime;

        float glow(float value, float center, float width) {
          return exp(-pow((value - center) / width, 2.0));
        }

        void main() {
          vec2 p = vUv - 0.5;
          float beam = glow(p.y + p.x * 0.22 + sin(uTime * 0.08) * 0.018, 0.04, 0.22);
          float oval = smoothstep(0.9, 0.12, length(p * vec2(1.0, 0.72)));
          float right = smoothstep(-0.34, 0.46, p.x);
          vec3 color = vec3(0.008, 0.018, 0.02) + vec3(0.12, 0.56, 0.62) * beam * 0.34;
          gl_FragColor = vec4(color, oval * right * 0.48);
        }
      `,
    }),
  );
}

function makeParticleMaterial() {
  return new THREE.ShaderMaterial({
    transparent: true,
    depthWrite: false,
    blending: THREE.AdditiveBlending,
    uniforms: {
      uTime: { value: 0 },
    },
    vertexShader: `
      attribute float aSize;
      attribute float aPhase;
      varying vec3 vColor;
      varying float vAlpha;
      uniform float uTime;

      void main() {
        vColor = color;
        vec3 pos = position;
        pos.z += sin(uTime * 0.45 + aPhase) * 0.035;
        pos.y += cos(uTime * 0.36 + aPhase * 0.7) * 0.018;

        vec4 mvPosition = modelViewMatrix * vec4(pos, 1.0);
        gl_PointSize = aSize * (320.0 / max(1.0, -mvPosition.z));
        vAlpha = smoothstep(-4.0, -1.2, mvPosition.z);
        gl_Position = projectionMatrix * mvPosition;
      }
    `,
    fragmentShader: `
      precision highp float;

      varying vec3 vColor;
      varying float vAlpha;

      void main() {
        vec2 p = gl_PointCoord - 0.5;
        float dist = length(p);
        if (dist > 0.5) {
          discard;
        }
        float core = smoothstep(0.5, 0.0, dist);
        float halo = exp(-dist * 7.5);
        gl_FragColor = vec4(vColor * (0.52 + core * 1.4), (core * 0.72 + halo * 0.22) * vAlpha);
      }
    `,
    vertexColors: true,
  });
}

function makePixelCloud() {
  const random = makeSeededRandom(22);
  const count = 440;
  const positions = [];
  const colors = [];
  const sizes = [];
  const phases = [];
  const cyan = new THREE.Color("#8cf4ff");
  const white = new THREE.Color("#f8fbff");
  const warm = new THREE.Color("#f0dbad");

  for (let index = 0; index < count; index += 1) {
    const layer = random();
    const x = -1.36 + random() * 2.72;
    const y = -0.78 + random() * 1.56;
    const z = -1.3 + layer * 2.65;
    const lane = Math.floor(random() * 9);
    const snappedY = y * 0.72 + (lane - 4) * 0.055;
    positions.push(x, snappedY, z);

    const color = random() > 0.9 ? warm : random() > 0.72 ? white : cyan;
    colors.push(color.r, color.g, color.b);
    sizes.push(0.045 + random() * 0.075 + layer * 0.02);
    phases.push(random() * Math.PI * 2);
  }

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute("color", new THREE.Float32BufferAttribute(colors, 3));
  geometry.setAttribute("aSize", new THREE.Float32BufferAttribute(sizes, 1));
  geometry.setAttribute("aPhase", new THREE.Float32BufferAttribute(phases, 1));

  const material = makeParticleMaterial();
  trackMaterial(material);
  return new THREE.Points(geometry, material);
}

function makePixelShards() {
  const random = makeSeededRandom(91);
  const count = 84;
  const geometry = new THREE.BoxGeometry(0.038, 0.038, 0.008);
  const material = new THREE.MeshBasicMaterial({
    color: 0xffffff,
    transparent: true,
    opacity: 0.46,
    blending: THREE.AdditiveBlending,
    depthWrite: false,
  });
  const mesh = new THREE.InstancedMesh(geometry, material, count);
  const matrix = new THREE.Matrix4();
  const position = new THREE.Vector3();
  const quaternion = new THREE.Quaternion();
  const scale = new THREE.Vector3();
  const rotation = new THREE.Euler();
  const cyan = new THREE.Color("#8cf4ff");
  const white = new THREE.Color("#f8fbff");
  const warm = new THREE.Color("#f0dbad");

  for (let index = 0; index < count; index += 1) {
    const depth = random();
    position.set(
      -0.72 + random() * 2.02,
      -0.68 + random() * 1.36,
      -0.78 + depth * 1.8,
    );
    rotation.set(
      (random() - 0.5) * 0.9,
      (random() - 0.5) * 1.4,
      (random() - 0.5) * Math.PI,
    );
    quaternion.setFromEuler(rotation);
    const tileScale = 0.7 + random() * 1.55 + depth * 0.7;
    scale.set(tileScale * (0.8 + random() * 1.9), tileScale * (0.62 + random() * 1.2), 1 + depth * 2.2);
    matrix.compose(position, quaternion, scale);
    mesh.setMatrixAt(index, matrix);

    const color = random() > 0.88 ? warm : random() > 0.66 ? white : cyan;
    mesh.setColorAt(index, color);
  }

  mesh.instanceMatrix.needsUpdate = true;
  if (mesh.instanceColor) {
    mesh.instanceColor.needsUpdate = true;
  }
  return mesh;
}

function addFrameCorner(group, x, y, xDir, yDir) {
  const z = 0.42;
  const lengthX = 0.52;
  const lengthY = 0.38;
  const radius = 0.012;
  const opacity = 0.78;
  const horizontal = makeTubeSegment(
    new THREE.Vector3(x, y, z),
    new THREE.Vector3(x + xDir * lengthX, y, z),
    radius,
    0x8cf4ff,
    opacity,
  );
  const vertical = makeTubeSegment(
    new THREE.Vector3(x, y, z),
    new THREE.Vector3(x, y + yDir * lengthY, z),
    radius,
    0x8cf4ff,
    opacity,
  );

  group.add(horizontal, vertical);
}

function makeCaptureFrame() {
  const group = new THREE.Group();
  const halfWidth = 1.5;
  const halfHeight = 0.88;
  addFrameCorner(group, -halfWidth, halfHeight, 1, -1);
  addFrameCorner(group, halfWidth, halfHeight, -1, -1);
  addFrameCorner(group, -halfWidth, -halfHeight, 1, 1);
  addFrameCorner(group, halfWidth, -halfHeight, -1, 1);

  const topRail = makeTubeSegment(
    new THREE.Vector3(-0.6, halfHeight, 0.36),
    new THREE.Vector3(0.6, halfHeight, 0.36),
    0.005,
    0xf8fbff,
    0.22,
  );
  const bottomRail = makeTubeSegment(
    new THREE.Vector3(-0.58, -halfHeight, 0.36),
    new THREE.Vector3(0.58, -halfHeight, 0.36),
    0.005,
    0xf8fbff,
    0.18,
  );
  group.add(topRail, bottomRail);
  return group;
}

function setGroupOpacity(group, opacity) {
  group.traverse((object) => {
    if (object.material) {
      object.material.opacity = opacity;
    }
  });
}

function makeVolumeLines() {
  const geometry = new THREE.BufferGeometry();
  const positions = [];
  const front = [
    new THREE.Vector3(-1.5, 0.88, 0.42),
    new THREE.Vector3(1.5, 0.88, 0.42),
    new THREE.Vector3(1.5, -0.88, 0.42),
    new THREE.Vector3(-1.5, -0.88, 0.42),
  ];
  const back = [
    new THREE.Vector3(-1.1, 0.58, -1.18),
    new THREE.Vector3(1.75, 0.7, -1.32),
    new THREE.Vector3(1.66, -0.68, -1.22),
    new THREE.Vector3(-1.18, -0.6, -1.08),
  ];

  front.forEach((point, index) => {
    const nextFront = front[(index + 1) % front.length];
    const backPoint = back[index];
    const nextBack = back[(index + 1) % back.length];
    positions.push(point.x, point.y, point.z, nextFront.x, nextFront.y, nextFront.z);
    positions.push(backPoint.x, backPoint.y, backPoint.z, nextBack.x, nextBack.y, nextBack.z);
    positions.push(point.x, point.y, point.z, backPoint.x, backPoint.y, backPoint.z);
  });

  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  return new THREE.LineSegments(
    geometry,
    new THREE.LineBasicMaterial({
      color: 0x8cf4ff,
      transparent: true,
      opacity: 0.18,
      blending: THREE.AdditiveBlending,
      depthWrite: false,
    }),
  );
}

function makeDataRibbon(yOffset, zOffset, color, opacity) {
  const points = Array.from({ length: 96 }, (_, index) => {
    const t = index / 95;
    const x = -1.76 + t * 3.9;
    const y = Math.sin((t - 0.12) * Math.PI * 1.24) * 0.18 + yOffset;
    const z = Math.sin(t * Math.PI * 1.4) * 0.38 + zOffset;
    return new THREE.Vector3(x, y, z);
  });

  return makeTube(points, 0.0065, color, opacity, 120);
}

function makeOrbit(radiusX, radiusY, z, color, opacity, start, end, tubeRadius) {
  const points = Array.from({ length: 120 }, (_, index) => {
    const t = index / 119;
    const angle = start + (end - start) * t;
    return new THREE.Vector3(Math.cos(angle) * radiusX, Math.sin(angle) * radiusY, z + Math.sin(angle * 1.7) * 0.08);
  });

  return makeTube(points, tubeRadius, color, opacity, 140);
}

const depthWash = new THREE.Mesh(new THREE.PlaneGeometry(6.6, 4.3), makeDepthWashMaterial());
depthWash.position.set(0.66, -0.06, -2.1);
depthWash.rotation.set(0.02, -0.16, 0.018);
volumeRig.add(depthWash);

const echoFrame = makeCaptureFrame();
echoFrame.name = "captureEchoFrame";
echoFrame.position.set(0.52, 0.62, -1.42);
echoFrame.rotation.set(-0.14, -0.5, -0.08);
echoFrame.scale.set(1.42, 1.24, 1);
setGroupOpacity(echoFrame, 0.13);
volumeRig.add(echoFrame);

const echoOrbit = makeOrbit(2.62, 1.48, -1.12, 0x8cf4ff, 0.1, -1.36, 1.16, 0.004);
echoOrbit.position.set(0.84, 0.48, -0.36);
echoOrbit.rotation.set(-0.18, -0.52, -0.22);
volumeRig.add(echoOrbit);

const surface = new THREE.Mesh(makeCaptureSurfaceGeometry(), makeCaptureSurfaceMaterial());
surface.name = "captureSurface";
surface.position.set(0, 0, 0);
surface.rotation.set(-0.08, -0.27, -0.045);
captureRig.add(surface);

const scanSheet = new THREE.Mesh(new THREE.PlaneGeometry(0.34, 2.35, 1, 24), makeScanSheetMaterial());
scanSheet.name = "scanSheet";
scanSheet.position.set(0.34, 0.02, 0.46);
scanSheet.rotation.set(-0.08, -0.27, -0.045);
captureRig.add(scanSheet);

const frame = makeCaptureFrame();
frame.name = "captureFrame";
frame.rotation.copy(surface.rotation);
captureRig.add(frame);

const pixelCloud = makePixelCloud();
pixelCloud.name = "pixelCloud";
pixelCloud.rotation.copy(surface.rotation);
captureRig.add(pixelCloud);

const pixelShards = makePixelShards();
pixelShards.name = "pixelShards";
pixelShards.rotation.copy(surface.rotation);
captureRig.add(pixelShards);

const volumeLines = makeVolumeLines();
volumeLines.name = "captureVolume";
volumeLines.rotation.copy(surface.rotation);
captureRig.add(volumeLines);

const ribbonTop = makeDataRibbon(0.66, -0.24, 0x8cf4ff, 0.24);
const ribbonCenter = makeDataRibbon(0.02, 0.1, 0x8cf4ff, 0.48);
const ribbonLower = makeDataRibbon(-0.68, -0.18, 0xf8fbff, 0.18);
[ribbonTop, ribbonCenter, ribbonLower].forEach((ribbon) => {
  ribbon.rotation.copy(surface.rotation);
  captureRig.add(ribbon);
});

const orbitOne = makeOrbit(1.88, 1.05, -0.04, 0x8cf4ff, 0.2, -1.18, 1.2, 0.006);
const orbitTwo = makeOrbit(2.2, 1.25, -0.28, 0xf0dbad, 0.12, -0.92, 1.0, 0.0045);
[orbitOne, orbitTwo].forEach((orbit) => {
  orbit.position.set(0.64, -0.02, -0.1);
  orbit.rotation.set(-0.1, -0.42, -0.2);
  orbitRig.add(orbit);
});

const ambientLight = new THREE.AmbientLight(0xffffff, 0.42);
const keyLight = new THREE.DirectionalLight(0xffffff, 1.9);
const cyanLight = new THREE.PointLight(0x8cf4ff, 3.2, 9.0);
const rimLight = new THREE.PointLight(0xf0dbad, 0.9, 7.0);

keyLight.position.set(-2.8, 2.6, 4.0);
cyanLight.position.set(2.4, 0.2, 2.0);
rimLight.position.set(1.5, -1.4, 2.6);
scene.add(ambientLight, keyLight, cyanLight, rimLight);

function resize() {
  const width = window.innerWidth;
  const height = window.innerHeight;
  const ratio = width / Math.max(height, 1);
  const narrow = width < 660;
  const portrait = ratio < 1.12;
  const tablet = width >= 660 && width < 980;

  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  renderer.setSize(width, height, false);
  camera.aspect = ratio;
  camera.fov = narrow ? 42 : 34;
  camera.updateProjectionMatrix();

  if (narrow) {
    layoutState.scale = 0.58;
    layoutState.x = 0.6;
    layoutState.y = -0.92;
  } else if (portrait) {
    layoutState.scale = 0.67;
    layoutState.x = 1.42;
    layoutState.y = -0.28;
  } else if (tablet) {
    layoutState.scale = 0.8;
    layoutState.x = 1.06;
    layoutState.y = -0.24;
  } else {
    layoutState.scale = 1.06;
    layoutState.x = 1.56;
    layoutState.y = 0.12;
  }

  stage.scale.setScalar(layoutState.scale);
  stage.position.set(layoutState.x, layoutState.y, 0);
}

function onPointerMove(event) {
  pointer.targetX = (event.clientX / window.innerWidth - 0.5) * 2;
  pointer.targetY = (event.clientY / window.innerHeight - 0.5) * -2;
}

function onScroll() {
  const max = Math.max(document.documentElement.scrollHeight - window.innerHeight, 1);
  scrollState.target = window.scrollY / max;
}

function revealElements() {
  const targets = [
    document.querySelector(".hero-copy"),
    document.querySelector(".hero-spec"),
    ...document.querySelectorAll(".section-inner > *"),
    ...document.querySelectorAll(".flow-steps article"),
  ].filter(Boolean);

  const observer = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          entry.target.classList.add("is-visible");
          observer.unobserve(entry.target);
        }
      });
    },
    { threshold: 0.14, rootMargin: "0px 0px -8% 0px" },
  );

  targets.forEach((target) => observer.observe(target));
}

function animate(time) {
  const seconds = time * 0.001;
  pointer.x += (pointer.targetX - pointer.x) * 0.055;
  pointer.y += (pointer.targetY - pointer.y) * 0.055;
  scrollState.value += (scrollState.target - scrollState.value) * 0.05;

  animatedMaterials.forEach((material) => {
    material.uniforms.uTime.value = seconds;
  });

  if (!reducedMotion) {
    captureRig.rotation.x = pointer.y * 0.03;
    captureRig.rotation.y = pointer.x * 0.07 + Math.sin(seconds * 0.18) * 0.018;
    captureRig.rotation.z = Math.sin(seconds * 0.16) * 0.012;
    volumeRig.rotation.x = pointer.y * 0.014;
    volumeRig.rotation.y = pointer.x * 0.035 + Math.sin(seconds * 0.1) * 0.012;
    orbitRig.rotation.y = pointer.x * 0.05 + seconds * 0.055;
    orbitRig.rotation.z = pointer.y * 0.025;

    scanSheet.position.x = Math.sin(seconds * 0.65) * 0.58 + 0.22;
    scanSheet.position.z = 0.48 + Math.cos(seconds * 0.65) * 0.08;
    pixelCloud.rotation.y = surface.rotation.y + Math.sin(seconds * 0.22) * 0.04;
    pixelShards.rotation.y = surface.rotation.y + Math.sin(seconds * 0.3) * 0.065;
    pixelShards.rotation.z = surface.rotation.z + Math.cos(seconds * 0.24) * 0.035;
    ribbonCenter.position.y = Math.sin(seconds * 0.42) * 0.026;
    ribbonCenter.position.z = Math.cos(seconds * 0.38) * 0.022;
  }

  stage.position.x = layoutState.x + pointer.x * 0.04;
  stage.position.y = layoutState.y - scrollState.value * 0.2 + pointer.y * 0.018;

  camera.position.x = pointer.x * 0.055;
  camera.position.y = 0.08 + pointer.y * 0.028;
  camera.lookAt(0.46, -0.04, 0);
  renderer.render(scene, camera);
  window.requestAnimationFrame(animate);
}

window.addEventListener("resize", resize, { passive: true });
window.addEventListener("pointermove", onPointerMove, { passive: true });
window.addEventListener("scroll", onScroll, { passive: true });

resize();
onScroll();
revealElements();
window.requestAnimationFrame(animate);
