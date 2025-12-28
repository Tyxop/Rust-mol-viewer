# PDB Visual - Motor Gráfico 3D para Visualización de Proteínas

Motor gráfico 3D de alto rendimiento en Rust para visualizar archivos PDB (Protein Data Bank), optimizado para modelos muy grandes con soporte para Vulkan y VR.

## Características

### ✅ Fase 1: Core & Parser (Completada)
- ✅ Parser PDB robusto basado en `nom`
  - Soporte ATOM, HETATM, HELIX, SHEET, CONECT
  - Inferencia automática de elementos químicos
  - Estructura de datos optimizada
- ✅ Motor de renderizado wgpu (Vulkan/Metal/DirectX 12)
- ✅ Sistema de cámara profesional (orbit, pan, zoom)
- ✅ Shaders WGSL con iluminación Phong
- ✅ Pipeline de depth buffer y alpha blending

### ✅ Fase 2: Representaciones Moleculares (Completada)
**4 modos de visualización implementados:**

1. **Van der Waals (Tecla 1)**
   - Esferas con radios atómicos de van der Waals
   - Esquema de colores CPK estándar
   - Instanced rendering de alta eficiencia

2. **Ball-and-Stick (Tecla 2)**
   - Esferas pequeñas para átomos
   - Cilindros para enlaces químicos
   - Inferencia automática de enlaces por distancia
   - Spatial partitioning con octree

3. **Ribbon/Cartoon (Tecla 3)**
   - Visualización de estructura secundaria
   - Hélices alpha como cintas helicoidales
   - Láminas beta como flechas direccionales
   - Splines suaves con geometría extruida

4. **Surface (Tecla 4)** - ¡NUEVO!
   - Superficie molecular SAS (Solvent-Accessible Surface)
   - Algoritmo marching cubes optimizado
   - Vertex welding para mallas continuas (~85% reducción de vértices)
   - Smoothing Laplaciano configurable
   - Semi-transparencia con alpha blending

### ✅ Fase 3: UI con egui (Completada)
- ✅ Estructura de UI modular (crate `mol-ui`)
- ✅ Panel de controles (selección de representación)
- ✅ Panel de información (stats proteína, FPS)
- ✅ Panel de configuración (ajustes visuales)
- ✅ Menú superior con Vista/Representación/Ayuda
- ✅ Integración completa con renderer
- ✅ Sistema de selección interactiva de átomos (ray picking + octree)

### ✅ Fase 4: Optimizaciones Avanzadas (Completada)
- ✅ Sistema LOD automático basado en distancia (5 niveles: High, Medium, Low, VeryLow, Impostor)
- ✅ Frustum culling con extracción de planos desde view-projection
- ✅ Octree espacial para búsquedas O(log n)
- ✅ **GPU Compute culling ACTIVADO** - 3-20x speedup según tamaño proteína
- ✅ **GPU extendido a Ball-and-Stick** - Modo híbrido (GPU átomos, CPU enlaces)
- ✅ Benchmarking CPU vs GPU en tiempo real (UI panel)
- ✅ Compute shaders para generación de superficies SDF (implementado)
- ✅ Parallel rendering con Rayon (SDF, inferencia de enlaces)
- ✅ Billboard impostors para átomos muy distantes
- ✅ Hysteresis en LOD para evitar popping visual

### ✅ Fase 5: Animaciones Moleculares (Completada)
- ✅ **Parser PDB Multi-Model** - Detección automática MODEL/ENDMDL
- ✅ **Estructura Trajectory** - Topología compartida + frames de coordenadas
- ✅ **Panel de Animación** - Play/Pause/Stop, slider de frames, control de FPS
- ✅ **Loop Modes** - Once, Loop, Ping-Pong (parcial)
- ✅ **Frame Update Loop** - Sistema de acumulación de tiempo
- ✅ **Actualización eficiente** - Solo posiciones (no topología)
- ✅ **Compatible con LOD/Culling** - GPU compute funciona con animaciones
- ✅ **Memoria optimizada** - ~95% menos que almacenar proteínas completas
- ⚠️ **Óptimo para Van der Waals** - Ball-and-Stick/Ribbon/Surface limitados

**Documentación:** Ver `FORMATOS_ANIMACION.md` y `ANIMATION_IMPLEMENTATION.md`

### ✅ Fase 6: VR con OpenXR (Completada)
- ✅ **Integración OpenXR** - Soporte completo para OpenXR 1.0+
- ✅ **Renderizado estéreo** - Dual render passes (left + right eye)
- ✅ **Input de controllers VR** - Sistema de actions para Oculus Touch
  - Joystick derecho: Rotar molécula
  - Joystick izquierdo: Mover cámara
  - Thumbstick click: Teleport
  - Grip button: Selección de átomos con ray picking
- ✅ **Sistema de cámara estéreo** - IPD automático y FOV asimétrico
- ✅ **UI en espacio 3D** - Paneles renderizados como quads 3D
- ✅ **Optimización 90 FPS** - Monitoreo de performance en tiempo real
- ✅ **LOD activo en VR** - Sistema de 5 niveles optimizado para VR
- ✅ **Compatibilidad multi-headset** - Quest 2/3, Index, Vive, WMR

**Lanzar en VR:**
```bash
cargo run --package mol-app --release -- --vr protein.pdb
```

**Documentación VR:**
- **Usuario**: Ver `docs/VR_USAGE.md` - Guía completa de uso
- **Desarrollador**: Ver `docs/VR_ARCHITECTURE.md` - Arquitectura técnica

### 🔜 Roadmap

**Fase 7: Features Avanzadas**
- Múltiples esquemas de color (CPK, cadena, estructura, hidrofobicidad)
- Medición de distancias y ángulos
- Exportación (PNG, OBJ, STL)
- Etiquetas y anotaciones
- Formatos DCD/XTC para trayectorias grandes
- Ball-and-Stick animation completa
- Interpolación de frames

## Instalación

### Prerrequisitos

1. **Instalar Rust** (si no está instalado):
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

2. **Verificar instalación**:
```bash
rustc --version
cargo --version
```

3. **Dependencias del sistema** (macOS):
```bash
# macOS viene con Metal por defecto, no se necesitan dependencias adicionales
```

**Dependencias del sistema** (Linux):
```bash
# Debian/Ubuntu
sudo apt install libwayland-dev libxkbcommon-dev

# Arch
sudo pacman -S wayland libxkbcommon

# Fedora
sudo dnf install wayland-devel libxkbcommon-devel
```

## Compilación

```bash
# Compilar en modo debug
cargo build

# Compilar en modo release (optimizado)
cargo build --release

# Compilar solo la aplicación principal
cargo build --package mol-app --release
```

## Uso

### Ejecutar con archivo PDB

```bash
# Modo debug
cargo run --package mol-app -- 9PZW.pdb

# Modo release (más rápido)
cargo run --package mol-app --release -- 9PZW.pdb
```

### Ejecutar sin archivo (esfera de prueba)

```bash
cargo run --package mol-app --release
```

### Controles

**Cámara:**
- **Ratón izquierdo + arrastrar**: Rotar cámara (orbit)
- **Ratón derecho + arrastrar**: Mover cámara (pan)
- **Rueda del ratón**: Zoom in/out
- **R**: Reset cámara (volver a vista inicial)

**Representaciones:**
- **1**: Van der Waals (esferas completas)
- **2**: Ball & Stick (enlaces)
- **3**: Ribbon (estructura secundaria)
- **4**: Surface (superficie molecular)

**General:**
- **ESC**: Salir de la aplicación

## Estructura del Proyecto

```
pdbvisual/
├── Cargo.toml                    # Workspace root
├── 9PZW.pdb                      # Archivo PDB de ejemplo (Receptor NMDA)
├── 6TAV.pdb                      # Archivo PDB adicional
├── README.md
├── QUICKSTART.md
├── crates/
│   ├── pdb-parser/              # Parser de archivos PDB
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── parser.rs        # Parser basado en nom
│   │   │   ├── structures.rs    # Atom, Protein, Bond, Chain
│   │   │   ├── bonds.rs         # Inferencia de enlaces
│   │   │   └── spatial.rs       # Octree para búsquedas espaciales
│   │   └── Cargo.toml
│   │
│   ├── mol-render/              # Motor de renderizado (wgpu)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── renderer.rs      # Renderer principal
│   │   │   ├── camera.rs        # Sistema de cámara orbit
│   │   │   ├── geometry.rs      # Generación de geometría base
│   │   │   ├── marching_cubes.rs # Algoritmo marching cubes
│   │   │   └── representations/ # Modos de visualización
│   │   │       ├── spheres.rs   # Van der Waals
│   │   │       ├── ball_stick.rs # Ball & Stick
│   │   │       ├── ribbon.rs    # Ribbon/Cartoon
│   │   │       └── surface.rs   # Superficie molecular
│   │   └── Cargo.toml
│   │
│   ├── mol-ui/                  # Interfaz de usuario (egui)
│   │   ├── src/
│   │   │   ├── lib.rs           # MolecularUI, UIState
│   │   │   └── panels.rs        # Paneles de UI
│   │   └── Cargo.toml
│   │
│   ├── mol-core/                # Utilidades compartidas (placeholder)
│   ├── mol-vr/                  # Integración OpenXR (placeholder)
│   │
│   └── mol-app/                 # Aplicación principal
│       ├── src/
│       │   └── main.rs          # Entry point, event loop
│       └── Cargo.toml
│
└── assets/
    └── shaders/                 # Shaders WGSL
        ├── sphere.wgsl          # Esferas VdW
        ├── ball_stick.wgsl      # Ball & Stick
        ├── ribbon.wgsl          # Ribbon
        └── surface.wgsl         # Superficie molecular
```

## Arquitectura

### PDB Parser
- Parser basado en `nom` para análisis robusto
- Soporte para records: ATOM, HETATM, HELIX, SHEET
- Estructura de datos optimizada con índices
- Inferencia de elementos químicos

### Renderer (wgpu)
- Abstracción sobre Vulkan/Metal/DirectX 12
- Instanced rendering para eficiencia
- Geometría icosphere con LOD preparada
- Pipeline de depth buffer para correcta oclusión
- Shaders WGSL con iluminación Phong

### Sistema de Cámara
- Cámara perspectiva con controles intuitos
- Orbit: rotación esférica alrededor del target
- Pan: movimiento lateral de cámara y target
- Zoom: acercamiento/alejamiento suave

## Información del Archivo de Ejemplo

**9PZW.pdb** - Receptor NMDA (GLUN1/GLUN2A)
- 27,525 átomos
- 8 cadenas (A, B, C, D, J, K, L, M)
- Múltiples hélices y láminas beta
- Resolución: 3.43 Å (microscopía electrónica)
- Tamaño: ~150×150×200 Å

**Rendimiento en Surface Mode (9PZW.pdb):**
- Grid: 150×271×226 vóxeles (9.2M)
- Generación: ~19.5 segundos
- Vértices finales: 354K (después de welding)
- Triángulos: 766K
- Memoria GPU: ~50 MB
- FPS: 60+ en hardware moderno

## Mejoras Técnicas Recientes

### Superficie Molecular (Diciembre 2025)
Se corrigieron **3 bugs críticos** que afectaban la visualización de superficie:

1. **Orden incorrecto del SDF** - El campo de distancia se calculaba en orden `x→y→z` pero se indexaba en orden `z→y→x`, destruyendo completamente la superficie.

2. **Chunking defectuoso** - El algoritmo descartaba triángulos que cruzaban límites de chunks, dejando solo ~20% de los triángulos visibles.

3. **Vértices NO compartidos** - Marching cubes generaba vértices duplicados (3 por triángulo) sin conectividad:
   - **Solución:** Implementación de vertex welding con spatial hashing (O(n))
   - **Resultado:** Reducción de 2.3M → 354K vértices (85% menos)
   - **Beneficio:** Malla continua con smoothing efectivo

### Algoritmo de Superficie
- **Tipo:** SAS (Solvent-Accessible Surface) - más continua que SES
- **Probe radius:** 1.4 Å (tamaño de molécula de agua)
- **Grid spacing:** 0.8 Å (equilibrio calidad/performance)
- **Smoothing:** Laplaciano, 2 iteraciones
- **Welding tolerance:** 0.08 Å (10% del grid spacing)

## Desarrollo

### Ejecutar tests

```bash
# Todos los tests
cargo test --workspace

# Tests de un crate específico
cargo test --package pdb-parser
```

### Linting

```bash
cargo clippy --workspace
```

### Formateo

```bash
cargo fmt --all
```

## Rendimiento

### Metas de Rendimiento (Fase 3+)

**Desktop (RTX 3060 equivalente):**
- Van der Waals: 60 FPS @ 27K átomos
- Ball-and-stick: 60 FPS @ 27K átomos + 30K enlaces
- Ribbon: 60 FPS @ 8 cadenas
- Surface: Generación <1s, render 60 FPS

**VR (Quest 2):**
- Todas las representaciones: 90 FPS stereo
- Latencia <20ms (motion-to-photon)

**Memoria:**
- GPU: <500 MB para 9PZW.pdb (todas las representaciones)
- RAM: <200 MB

## Tecnologías

**Core:**
- **Rust 2021** - Lenguaje base con seguridad de memoria
- **wgpu 22.0** - Abstracción GPU moderna (Vulkan/Metal/DX12)
- **winit 0.30** - Gestión multiplataforma de ventanas y eventos

**Matemáticas & Geometría:**
- **glam 0.29** - Matemáticas vectoriales SIMD-optimized
- **nalgebra 0.33** - Álgebra lineal (splines, transformaciones)

**Parsing & Datos:**
- **nom 7.1** - Parser combinators para formato PDB
- **bytemuck** - Zero-cost conversiones de tipos para GPU
- **serde** - Serialización de configuraciones

**Paralelización:**
- **rayon 1.10** - Paralelización de datos (SDF, smoothing)
- **parking_lot 0.12** - Sincronización de baja latencia

**UI (En desarrollo):**
- **egui 0.29** - UI immediate-mode
- **egui-wgpu 0.29** - Backend wgpu para egui
- **egui-winit 0.29** - Integración con winit

**Futuro:**
- **OpenXR 0.19** (Fase 4) - VR multiplataforma estándar

## Estado del Proyecto

### ✅ Completado
- [x] **Fase 1-2:** Parser PDB completo con soporte ATOM, HELIX, SHEET, CONECT
- [x] **Fase 1-2:** Motor de renderizado moderno con wgpu (Vulkan/Metal/DirectX)
- [x] **Fase 2:** 4 representaciones moleculares (VdW, Ball&Stick, Ribbon, Surface)
- [x] **Fase 1:** Sistema de cámara profesional orbit/pan/zoom
- [x] **Fase 2:** Inferencia de enlaces químicos con spatial partitioning
- [x] **Fase 2:** Superficie molecular con marching cubes + vertex welding
- [x] **Fase 3:** UI completa con egui (paneles + menús + integración)
- [x] **Fase 3:** Sistema de selección interactiva (ray picking + octree)
- [x] **Fase 4:** Sistema LOD automático (5 niveles con hysteresis)
- [x] **Fase 4:** Frustum culling con extracción de planos
- [x] **Fase 4:** Octree espacial para búsquedas O(log n)
- [x] **Fase 4:** Compute shaders GPU (culling, SDF) - modo experimental
- [x] **Fase 4:** Paralelización CPU con Rayon (surface, bonds)
- [x] **Fase 1-4:** Shaders WGSL optimizados con iluminación Phong

### 🔨 En Desarrollo
- [ ] Medición de distancias entre átomos seleccionados
- [ ] Medición de ángulos (3 átomos)
- [ ] Múltiples esquemas de color (cadena, residuo, hidrofobicidad)

### 📋 Pendiente (Fase 5+)
- [ ] **Fase 5:** Integración OpenXR para VR
- [ ] **Fase 5:** Renderizado estéreo para VR
- [ ] **Fase 5:** Input de controllers VR
- [ ] **Fase 6:** Exportación de imágenes (PNG, screenshots)
- [ ] **Fase 6:** Exportación de geometría (OBJ, STL)
- [ ] **Fase 6:** Soporte para trayectorias MD (animaciones)
- [ ] **Fase 6:** Soporte para formatos adicionales (mmCIF, MOL2)
- [ ] **Fase 6:** Etiquetas y anotaciones 3D

### 🐛 Bugs Conocidos
- La generación de superficie puede ser lenta para proteínas muy grandes (>100K átomos)

### 🔧 Bugs Recientemente Resueltos
- ✅ **Normales invertidas en Surface** - Las normales ahora se calculan usando gradiente del SDF (más preciso y consistente)
- ✅ **Artefactos de transparencia en Surface** - Superficie ahora completamente opaca (alpha=1.0) para evitar artefactos de depth write + alpha blending

### 🎯 Próximos Pasos Sugeridos
1. **Medición de distancias** - Mostrar distancia entre 2 átomos seleccionados + línea 3D
2. **Medición de ángulos** - Calcular ángulos entre 3 átomos seleccionados
3. **Esquemas de color múltiples** - Por cadena, por residuo, por hidrofobicidad
4. **Centro en selección** - Animar cámara hacia centroide de átomos seleccionados
5. **Timestamp queries GPU** - Medir tiempo real de ejecución GPU (no solo dispatch)
6. **Preparar integración VR** - Investigar OpenXR + pruebas con Quest

## Licencia

MIT License (pendiente de añadir archivo LICENSE)

## Autor

Desarrollado con Rust, wgpu, y dedicación para la visualización científica.

## Referencias

- [Protein Data Bank (RCSB)](https://www.rcsb.org/)
- [PDB File Format Specification](https://www.wwpdb.org/documentation/file-format)
- [wgpu - Modern GPU API](https://wgpu.rs/)
- [egui - Immediate Mode GUI](https://www.egui.rs/)
- [Marching Cubes Algorithm](https://en.wikipedia.org/wiki/Marching_cubes)
- [OpenXR Specification](https://www.khronos.org/openxr/)
- [PyMOL](https://pymol.org/) - Inspiración para representaciones moleculares
