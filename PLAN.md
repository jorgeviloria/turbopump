# Turbopump — plan de trabajo

Software libre y multiplataforma (Windows, macOS, Linux), escrito en Rust, para el
diseño preliminar e interactivo de turbomáquinas: bombas, ventiladores, compresores
y turbinas. El objetivo es una herramienta completa: del punto de diseño a la
geometría 3D, los sólidos CAD y la preparación para CFD.

> "Turbopump" es un nombre provisional.

---

## 1. Reglas del proyecto (diseño en sala limpia)

Para que el proyecto pueda publicarse como código abierto sin riesgo de que lo
retiren, todo se reimplementa a partir de fuentes públicas:

| Se puede usar | No se usa |
|---|---|
| Libros y artículos publicados: Gülich, Stepanoff, Pfleiderer, Aungier, Casey/Robinson, Balje, Lieblein, Stanitz/Prian, Wiesner… | Decompilar o desensamblar programas comerciales |
| Geometrías públicas de referencia (Eckardt, Krain, NASA Rotor 37, bomba ERCOFTAC, Francis-99) | Copiar textos, imágenes, iconos o interfaces de manuales o programas propietarios |
| Librerías libres de calidad industrial (ver §3) | Formatos o kernels propietarios, como Parasolid |

Las ecuaciones y los métodos de ingeniería no tienen copyright; el código y los
textos de otros sí. Cada fórmula del código llevará la referencia bibliográfica de
la que sale.

---

## 2. Funciones que tendrá la aplicación

### 2.1 Tipos de máquina
- **Bombas:** centrífugas, de flujo mixto, axiales, inductores, de aguas residuales y Barske (nq bajo).
- **Ventiladores:** centrífugos, de jaula de ardilla, axiales y de refrigeración de automoción.
- **Compresores:** centrífugos y axiales.
- **Turbinas de gas:** de flujo radial hacia dentro y axiales (incluidas las de motor cohete).
- **Turbinas hidráulicas:** Francis y Kaplan.

### 2.2 Proyecto y componentes
- Un proyecto describe una máquina de una o varias etapas: hasta 5 rodetes, cualquier número de estatores (con o sin álabes) y una voluta al principio o al final.
- Acoplamiento geométrico entre componentes, en sentido del flujo, en sentido inverso o desacoplados.
- Rodetes contrarrotantes y velocidades alternativas.
- Recálculo automático en cadena cuando cambia un paso anterior.
- Deshacer y rehacer con historial.
- Comparación de varios diseños a la vez y diseños de referencia.

### 2.3 Configuración global
- **Punto de diseño:** caudal (volumétrico o másico); altura, Δp, relación de presiones o trabajo específico; velocidad de giro.
- **Fluido y condiciones de entrada:** prerrotación y sentido de giro.
- **Cavitación:** NPSH, número de Thoma y altura de instalación.
- **Base de fluidos:** fluidos incompresibles, gas ideal, fluidos reales de **CoolProp** (puros, mezclas con agua y bifásicos), mezclas de gases y REFPROP opcional, si el usuario tiene licencia propia.

### 2.4 Rodete (impeller): pasos de diseño
1. **Dimensiones principales:** teoría de línea de corriente 1D, ecuación de Euler y triángulos de velocidad. Incluye:
   - diagramas de Cordier y Balje y velocidad específica;
   - eficiencias (hidráulica, volumétrica, mecánica, por fricción de disco y por holgura de punta);
   - diámetro del eje a partir del esfuerzo de torsión y diseño en mid-span.
2. **Contorno meridional:**
   - cubo y carcasa definidos con Bézier, segmentos de recta y arco, o B-spline;
   - bordes de ataque y de salida;
   - cálculo del flujo meridional (función de corriente y flujo potencial);
   - material de cubo y carcasa, y trayectoria de flujo secundaria.
3. **Propiedades del álabe:**
   - número de álabes y álabes divisores (splitters);
   - espesores y tipo de superficie: reglada, de elemento radial o compuesta;
   - spans, distribución de cu/cm y equilibrio radial;
   - ángulos de entrada y salida, incidencia y bloqueo;
   - deslizamiento (slip) según Wiesner/Gülich, Aungier/Wiesner, Pfleiderer o von Backström.
4. **Líneas medias del álabe:**
   - modos de diseño: mapeo conforme (t, m), distribución de β, carga del álabe B′ (diseño inverso), proyección periférica o geometría fija;
   - vistas auxiliares: ángulo de inclinación (lean), velocidad de impacto (bombas aptas para peces), velocidades sobre el álabe (Stanitz-Prian) y flujo álabe a álabe en 1D y 2D.
5. **Perfiles del álabe:** distribuciones de espesor lineal, constante, definida por el usuario o con estrechamiento en el borde de salida.
6. **Bordes del álabe:** elíptico, redondeado, recto o "cortado".
7. **Álabes tipo perfil aerodinámico (máquinas axiales):**
   - perfiles NACA y otros de la base de datos;
   - cinemática por BEM, Lieblein o método simple;
   - barrido (sweep) y apilado.
8. **Preparación para CFD:** extensiones de geometría virtual y conexiones de interfaz rotor-estator (RSI).
9. **Acabado del modelo:**
   - redondeos entre álabe y cubo o carcasa;
   - recorte del rodete;
   - agujeros de equilibrado;
   - álabe extendido;
   - sólidos del dominio fluido y del dominio de material.

### 2.5 Estator
- Dimensiones principales, contorno meridional, número de álabes (con chequeo de periodicidad e interferencia), líneas medias, perfiles, bordes, CFD y acabado.
- Tipos de líneas medias: circular más forma libre, espiral logarítmica más tramo recto y difusor radial.

### 2.6 Voluta
- **Tipo y entrada:** voluta de salida o de entrada.
- **Secciones transversales:** Bézier, segmentos de recta, basadas en radio, rectángulo, trapecio o círculo; simétricas o asimétricas; internas o externas.
- **Áreas de desarrollo de la espiral:**
  - reglas de Pfleiderer, Stepanoff y momento angular constante;
  - compensación de la lengüeta y aproximación por arcos circulares;
  - voluta doble.
- **Difusor de salida:** recto, curvo o tangencial, con progresión de áreas.
- **Lengüeta (cut-water):** simple, con redondeo o en arista viva.
- Espesor de pared y geometría para CFD.

### 2.7 Análisis
- **Predicción de prestaciones:**
  - curva de Euler menos pérdidas, para todas las máquinas;
  - Casey/Robinson para compresores;
  - límites de bombeo (surge) y bloqueo (choke).
- **Funciones de aproximación:** unas 150 correlaciones empíricas que el usuario puede ver y editar. Cada parámetro funciona en modo **automático** (propuesto por la correlación) o **manual**.
- **Informe de diseño** en HTML, CSV y TXT.

### 2.8 Importación
- Ingeniería inversa 3D a partir de STEP, IGES o BREP: ajustar un diseño paramétrico a una geometría existente.
- Datos de puntos 2D.
- Formato RTZT (r, z, θ y espesor por span).
- Formato propio de intercambio.

### 2.9 Exportación
- **Neutros:** STEP, IGES, BREP, STL, DXF y geometría general en TXT o XML.
- **Malla tetraédrica:** Fluent `.msh`, OpenFOAM `polyMesh`, Netgen `.vol` y Abaqus `.inp`.
- **CFD:**
  - Ansys TurboGrid (curvas de cubo, carcasa y perfil) y ANSYS ICEM;
  - CFD-Pre XML;
  - caso OpenFOAM completo, que es la prioridad por ser libre;
  - Simerics, Pointwise, AutoGrid y STAR-CCM+ (puntos y superficies).
- **CAD:** con STEP se cubren SolidWorks, Inventor, Creo, CATIA y NX. Las macros específicas de cada programa quedan como opcionales.
- Datos de prestaciones.

### 2.10 Modo por lotes y optimización
- Ejecutable de línea de comandos sin interfaz gráfica:
  ```
  turbopump batch proyecto.tpb --export step --log run.log
  ```
  Devuelve los códigos de salida 0 (correcto), 1 (avisos) y 2 (errores).
- Archivo de lote con parámetros modificables y rangos, para DoE u optimizadores como Dakota u OpenMDAO.
- Bindings de Python (`pyo3`) para scripting.

### 2.11 Interfaz
- Árbol de proyecto y lista de componentes.
- Asistente por pasos con estado de cada paso: pendiente, válido o con avisos.
- Diagramas interactivos en los que los puntos de control Bézier se arrastran con el ratón.
- Vista meridional, vista 3D (sólido, alambre o transparencia; árbol del modelo) y vista de informe.
- Unidades SI o US, varias definiciones de velocidad específica y preferencias.

### Fuera de alcance o para más adelante
- La integración con el Explorador de Windows; se sustituye por asociaciones de archivo en cada sistema operativo.
- La exportación a Parasolid, que es un formato propietario.
- El análisis estructural (FEA) y las curvas de prestaciones por CFD. Podrían añadirse en una fase posterior con CalculiX y OpenFOAM.

---

## 3. Librerías de terceros

Todas son libres salvo REFPROP, que es opcional:

| Librería | Para qué | En Turbopump (Rust) |
|---|---|---|
| **Open CASCADE (OCCT)** | Kernel CAD: superficies B-spline, sólidos, cosido, booleanas, redondeos, STEP/IGES | El mismo OCCT 7.8.1, compilado de forma estática con `occt-sys`, a través de un puente C++ propio (`cxx`) que convierte las excepciones en errores |
| **netgen** | Mallas tetraédricas | El mismo netgen, mediante FFI a `nglib` generada con `bindgen` |
| **CoolProp** | Propiedades de fluidos reales | El mismo CoolProp, cargado al ejecutar con un cargador propio (`libloading` sobre su API en C). Si falta, el agua usa un modelo IAPWS propio |
| **AMD AOCL** (BLAS/LAPACK) | Álgebra lineal | `faer` / `nalgebra`, en Rust puro. Así evitamos una dependencia nativa y funciona también en ARM (Mac M) |
| REFPROP (opcional) | Fluidos NIST | Backend de REFPROP dentro de CoolProp, si el usuario lo tiene instalado |

Resto del stack en Rust:

| Área | Herramienta |
|---|---|
| Interfaz gráfica | `eframe`/`egui` + `egui_dock` (paneles acoplables) + `egui_plot` |
| Visor 3D | `wgpu`, con la malla del propio diseño, que sigue cada cambio al instante; la teselación de OCCT se añadirá para ver el acabado |
| Unidades | `tp-units` propio: SI interno y conversión en tiempo de ejecución, porque la interfaz deja elegir unidades (descartado `uom`, que fija las unidades al compilar) |
| Formato de proyecto | `serde` (JSON con versión de esquema) |
| XML | `quick-xml` |
| Mallas y CAD ligero | `stl_io`, `dxf` |
| Ajuste y optimización | `argmin` |
| Pruebas | `proptest` (propiedades) y `insta` (golden files) |
| Distribución | `cargo-dist` / `cargo-packager`: MSI, DMG firmado y AppImage/Flatpak |
| Integración continua | GitHub Actions con Windows, macOS (x86 y ARM) y Linux |

Licencia recomendada: **Apache-2.0 o MIT**, la convención en Rust. OCCT
(LGPL-2.1 con excepción) y netgen (LGPL-2.1) se enlazan respetando la LGPL. Si
prefieres obligar a que los forks sigan siendo libres, la opción es GPL-3.0.

---

## 4. Arquitectura

Es un workspace de Cargo. El motor de diseño no depende de la interfaz: la misma
lógica la usan la GUI, la línea de comandos y Python.

```
turbopump/
├── crates/
│   ├── tp-units/     Magnitudes con unidades, SI/US, definiciones de nq
│   ├── tp-math/      Bézier, B-spline/NURBS, polilíneas, interpolación,
│   │                 integración, raíces (sin dependencias nativas)
│   ├── tp-fluids/    Fluidos: incompresible, gas ideal, CoolProp, REFPROP
│   ├── tp-design/    Núcleo de turbomaquinaria (Rust puro, 100 % testeable):
│   │   ├── correlations/  ~150 funciones de aproximación (editables)
│   │   ├── impeller/      dimensiones principales por tipo de máquina
│   │   ├── meridian/      contorno + cálculo de flujo meridional
│   │   ├── blade/         propiedades, líneas medias, perfiles, bordes,
│   │   │                  slip, equilibrio radial, Stanitz-Prian, B2B
│   │   ├── airfoil/       NACA, BEM, Lieblein
│   │   ├── stator/  volute/
│   │   └── performance/   Euler+pérdidas, Casey/Robinson
│   ├── tp-project/   Modelo de proyecto, componentes, acoplamientos,
│   │                 grafo de dependencias de pasos, undo/redo, serialización
│   ├── tp-cad/       Puente con OCCT: superficies, sólidos, acabado,
│   │                 STEP/IGES/BREP/STL, teselación para el visor
│   ├── tp-mesh/      Puente con netgen + escritores Fluent/OpenFOAM/vol/inp
│   ├── tp-io/        Importación/exportación (geo-txt/xml, RTZT, TurboGrid,
│   │                 CFD-Pre XML, caso OpenFOAM, DXF, informes)
│   ├── tp-cli/       Modo por lotes (códigos de salida 0/1/2)
│   ├── tp-py/        Bindings de Python (pyo3)
│   └── tp-gui/       Aplicación egui + visor wgpu
├── data/             Correlaciones, perfiles NACA, fluidos por defecto
├── tests/benchmarks/ Casos de validación (Gülich, Eckardt, Krain, Rotor 37…)
└── docs/             Manual propio y teoría con referencias
```

Conceptos clave del diseño:

- **Parámetro dual.** Cada parámetro es `Auto(correlación)` o `Manual(valor)`.
  Así se reproduce el diseño "semiautomático": el programa propone un valor y el
  usuario puede sobrescribirlo.
- **Grafo de pasos.** Cada componente es una cadena de pasos (dimensiones →
  meridiano → propiedades → líneas medias → perfiles → bordes → CFD → acabado).
  Cambiar un paso invalida y recalcula solo los pasos que dependen de él.
- **Geometría en dos niveles.** `tp-design` genera nubes de puntos y curvas en
  Rust puro, que es rápido y testeable. Solo `tp-cad` convierte eso en sólidos
  OCCT. Así el núcleo compila y se prueba sin C++.
- **Validación numérica.** Cada fórmula tiene un test contra un ejemplo resuelto
  del libro del que procede, y cada tipo de máquina un benchmark geométrico
  público.

---

## 5. Fases de trabajo

Las estimaciones suponen una persona a tiempo completo trabajando conmigo. Es un
proyecto grande, así que se prioriza tener pronto algo útil:
primero la bomba centrífuga, que es la mejor documentada (Gülich), y después el
resto de máquinas.

| Fase | Contenido | Entregable | Duración |
|---|---|---|---|
| **0. Base** ✅ | Workspace, CI en los 3 sistemas, licencia, `tp-units`, `tp-math` (Bézier/B-spline), modelo de proyecto, serialización, undo/redo | Repositorio que compila y pasa tests en Win/Mac/Linux | Hecha |
| **1. Diseño 1D de bomba centrífuga** ✅ | Configuración global, fluidos (CoolProp), dimensiones principales, triángulos de velocidad, Cordier, slip, eficiencias, eje | CLI que calcula las dimensiones principales. Hecha; quedan por verificar varios coeficientes de Gülich | Hecha |
| **2. Geometría del rodete** ✅ | Contorno meridional, flujo meridional (cuasi-1D), propiedades del álabe, líneas medias (mapeo conforme y β), espesores, bordes, superficies 3D en puntos | Hecha: puntos 3D, malla STL y vistas SVG. El flujo potencial 2D pasa a la Fase 7 y TurboGrid a la Fase 8 | Hecha |
| **3. CAD con OCCT** ✅ | Puente C++ propio con OCCT 7.8.1 estático, superficies B-spline, sólidos del rodete y del dominio fluido, STEP/IGES/STL | Hecha: sólidos válidos verificados por volumen; el disco del cubo es macizo hasta la Fase 9 | Hecha |
| **4. GUI MVP** ✅ | Ventana, árbol, pasos, diálogos, diagramas con Bézier arrastrables, visor 3D | **v0.1: diseño completo de un rodete de bomba centrífuga, exportable a STEP.** Hecha con egui 0.36 y wgpu; quedan para fases posteriores los diagramas de β arrastrables, el editor de correlaciones y la teselación de OCCT en el visor | Hecha |
| **5. Voluta y estator** ✅ | Voluta completa, estatores con y sin álabes, acoplamiento entre componentes, multietapa | **v0.2: bomba completa (rodete + voluta).** Hecha: voluta de salida (secciones rectangulares, trapezoidales o circulares; momento angular o velocidad constante; tobera y lengüeta), difusores con y sin álabes, acoplamiento con invalidación aguas abajo, bombas multietapa con la altura repartida entre los rodetes, sólidos CAD, línea de comandos e interfaz. Después se añadieron la voluta doble con su nervio, voluta interior y canal exterior, también en CAD; el empuje radial de la tabla 9.4 de Gülich; la descarga radial o central (ec. 7.29) y la tangencial con lado recto elegible; la lengüeta afilada o redondeada con su nariz en d_z; el nervio de la voluta doble con nariz y cola elípticas, tallado en una espiral única; y la descarga vertical por defecto, con la tangencial abierta a ambos lados y el codo radial contado en la longitud. Quedan para más adelante los canales de retorno entre etapas, las volutas gemelas y de entrada, la voluta tras otra voluta, las reglas de Pfleiderer y Stepanoff y el sólido de la carcasa con su espesor de pared | Hecha |
| **6. Resto de máquinas** | Ventiladores (incl. axiales con perfiles, BEM y Lieblein), compresores centrífugos y axiales, turbinas radiales y axiales, Francis y Kaplan; editor de correlaciones | v0.3–0.5 | 3–4 meses |
| **7. Análisis** ✅ | Predicción de prestaciones (Euler y Casey/Robinson), flujo meridional potencial 2D, álabe a álabe 1D/2D, velocidades sobre el álabe, informe | **v0.6.** Hecha para bombas: curva característica (Euler con deslizamiento, rendimiento hidráulico de Gülich y zona de recirculación estimada hasta válvula cerrada), flujo potencial meridional por elementos finitos, velocidades en las caras del álabe por la circulación, vista de curvas, informe y CSV. Después se añadieron la curva NPSH₃(Q) (Gülich cap. 6.3.3) y las pérdidas de la voluta frente al caudal (Japikse cap. 4.3), validadas con el ejemplo del cap. 7.5 de Japikse. Quedan para más adelante Casey/Robinson y los límites de bombeo y bloqueo (con los compresores de la Fase 6), el análisis álabe a álabe en 2D, el flujo potencial como base del diseño de álabes y el informe en HTML | Hecha |
| **8. Malla y CFD** | netgen + escritores, geometría virtual, RSI, caso OpenFOAM, CFD-Pre XML y curvas de TurboGrid | v0.7 | 4–6 sem |
| **9. Producción** | Acabado (redondeos, recorte, agujeros de equilibrado), modo por lotes, Python, ingeniería inversa desde STEP, instaladores firmados, manual | **v1.0** | 2–3 meses |

**Totales aproximados:** un MVP útil (rodete y voluta de bomba con exportación a
STEP) en unos **5–6 meses**, y todas las funciones del plan en unos **18–24 meses**.

---

## 6. Cómo se trabaja cada fase

1. **Especificación.** Para cada paso de diseño escribo una nota en `docs/theory/`
   con sus entradas, salidas, ecuaciones y referencias bibliográficas. Es nuestra
   propia especificación.
2. **Tests primero.** Los ejemplos resueltos de los libros y los benchmarks
   públicos se convierten en tests antes de implementar.
3. **Implementación** en `tp-design`, en Rust puro y sin GUI.
4. **Integración.** Se conecta el paso al proyecto, a la línea de comandos y a la
   interfaz.
5. **Verificación visual** con la propia aplicación y con FreeCAD/ParaView, que
   son libres.
6. **Commits pequeños por funcionalidad.** Tú los revisas antes de avanzar.

---

## 7. Lo primero que hay que hacer

1. **No subir al repositorio** instaladores ni manuales de terceros. El `.gitignore`
   ya los excluye.
2. Elegir el nombre definitivo y la licencia.
3. Arrancar la **Fase 0**.
