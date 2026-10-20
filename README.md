# Turbopump

Turbopump es una aplicación libre y multiplataforma (Windows, macOS y Linux), escrita en
Rust, para el diseño preliminar e interactivo de turbomáquinas: bombas, ventiladores,
compresores y turbinas.

> **Estado:** Fase 4, versión 0.1: diseño completo del rodete de una bomba centrífuga,
> desde el punto de diseño hasta los sólidos CAD (STEP, IGES, STL), con interfaz gráfica
> y línea de comandos. La hoja de ruta completa está en [PLAN.md](PLAN.md).

![Editor de Turbopump](docs/img/editor.jpg)

## Qué hay ahora

| Crate | Contenido |
|---|---|
| [`tp-units`](crates/tp-units) | Magnitudes físicas, unidades SI/US, conversión y lectura de texto (`"2900 rpm"`), y velocidad específica (nq, ns US, ωs, σ de Cordier) |
| [`tp-math`](crates/tp-math) | Curvas Bézier y B-spline (evaluación, derivadas, subdivisión, interpolación), polilíneas, interpolación 1D monótona, raíces y cuadratura |
| [`tp-fluids`](crates/tp-fluids) | Propiedades de líquidos con CoolProp, cargado al ejecutar, y un modelo propio del agua (IAPWS) para cuando falta |
| [`tp-design`](crates/tp-design) | Triángulos de velocidad, deslizamiento (Wiesner), correlaciones de bomba, dimensiones principales y geometría del rodete: contorno meridional, flujo meridional, álabes y espesores |
| [`tp-io`](crates/tp-io) | Exportación: lista de puntos, malla STL y dibujos SVG |
| [`tp-cad`](crates/tp-cad) | Sólidos del rodete y del dominio fluido con Open CASCADE, y exportación STEP, IGES y STL |
| [`tp-project`](crates/tp-project) | Modelo de proyecto, parámetros automáticos o manuales, componentes y sus reglas, pasos de diseño, archivo `.tpj` y deshacer/rehacer |
| [`tp-cli`](crates/tp-cli) | Ejecutable `turbopump` para la línea de comandos |
| [`tp-gui`](crates/tp-gui) | Ejecutable `turbopump-gui`: editor gráfico con egui y vista 3D con wgpu |

## Compilar y probar

Hace falta:

- Rust 1.95 o posterior;
- **cmake**;
- un **compilador C++17** (Xcode Command Line Tools, GCC/Clang o Visual Studio).

La primera compilación construye Open CASCADE 7.8.1 desde el código fuente y tarda de 5 a
15 minutos; las siguientes lo reutilizan. Para compilar sin CAD:

```bash
cargo build -p tp-cli --no-default-features
```

```bash
cargo build -p tp-gui --no-default-features
```

```bash
cargo build --release
```

```bash
cargo test --workspace
```

## Interfaz gráfica

```bash
cargo run --release -p tp-gui
```

También abre un proyecto existente:

```bash
cargo run --release -p tp-gui -- bomba.tpj
```

Arranca con una bomba de 100 m³/h, 32 m y 2900 rpm. El trabajo sigue los pasos de diseño
del rodete:

- **Árbol del proyecto** (izquierda): configuración global y pasos de cada componente,
  con su estado: pendiente (○), hecho (⏺) o desactualizado (⟲) porque cambió un dato
  anterior.
- **Panel del paso** (derecha): los datos del paso seleccionado. Cada valor es
  automático (propuesto por una correlación) o manual. *Accept step* marca el paso como
  hecho y pasa al siguiente.
- **Vistas** (centro, acoplables):
  - vista meridional, donde los puntos de control de Bézier, el borde de ataque y la
    longitud axial se arrastran con el ratón;
  - gráficas del álabe (β, θ y espesor);
  - vista frontal;
  - vista 3D;
  - informe con las mismas cifras que la línea de comandos.
- **Edición:** todo se puede deshacer y rehacer (Cmd/Ctrl+Z, Cmd/Ctrl+Shift+Z). Un
  arrastre completo cuenta como un solo paso.
- **Exportación** (menú *File ▸ Export*): puntos, malla STL, dibujos SVG y sólidos CAD
  STEP, IGES o STL, que se construyen en segundo plano.

Las pruebas de la interfaz usan [kittest](https://github.com/rerun-io/kittest) y no
necesitan GPU. Las capturas con wgpu se generan a mano en `target/screenshots/`:

```bash
cargo test -p tp-gui --test screenshots -- --ignored
```

## Ejemplos de uso

Velocidad específica de una bomba de 100 m³/h, 32 m de altura y 2900 rpm:

```bash
cargo run -p tp-cli -- nq --flow "100 m3/h" --head "32 m" --speed "2900 rpm"
```

Crear un proyecto nuevo y ver su resumen:

```bash
cargo run -p tp-cli -- new bomba.tpj --machine pump --flow "100 m3/h" --head "32 m" --speed "2900 rpm"
```

```bash
cargo run -p tp-cli -- info bomba.tpj
```

Calcular las dimensiones principales del rodete. El resultado se guarda en el proyecto, y
cualquier valor se puede fijar con `--set` (los números sin unidad usan las del proyecto):

```bash
cargo run -p tp-cli -- design bomba.tpj
```

```bash
cargo run -p tp-cli -- design bomba.tpj --set d2=180 --set z=6
```

Construir la geometría del rodete (contorno meridional, flujo meridional, álabes,
espesores y bordes) y exportar puntos, malla STL y dibujos:

```bash
cargo run -p tp-cli -- geometry bomba.tpj --export salida
```

| Vista meridional | Vista frontal |
|---|---|
| ![meridional](docs/img/rodete-referencia-meridional.svg) | ![frontal](docs/img/rodete-referencia-frontal.svg) |

Sólidos CAD del rodete y del dominio fluido (Open CASCADE):

```bash
cargo run -p tp-cli -- cad bomba.tpj --format step,iges,stl
```

| Rodete | Dominio fluido |
|---|---|
| ![rodete](docs/img/rodete-referencia-cad.jpg) | ![fluido](docs/img/fluido-referencia-cad.jpg) |

Propiedades de un líquido:

```bash
cargo run -p tp-cli -- fluid Water --temperature "60 °C"
```

Convertir unidades:

```bash
cargo run -p tp-cli -- convert 150 psi bar
```

## CoolProp

Turbopump usa [CoolProp](http://www.coolprop.org) para las propiedades de los fluidos y lo
carga al ejecutarse. Busca `libCoolProp.dylib` (macOS), `libCoolProp.so` (Linux) o
`CoolProp.dll` (Windows) en este orden:

1. la ruta de la variable `TURBOPUMP_COOLPROP`;
2. junto al ejecutable;
3. en el sistema.

Si no la encuentra, el agua sigue disponible con un modelo propio basado en IAPWS.
`TURBOPUMP_COOLPROP=none` desactiva CoolProp a propósito.

Durante el desarrollo se puede usar la copia que descarga `cargo` para las pruebas:

```bash
export TURBOPUMP_COOLPROP="$(cargo run -q -p tp-fluids --example coolprop_path)"
```

## Licencia

Puedes usar el proyecto bajo cualquiera de estas dos licencias, a tu elección:

- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT ([LICENSE-MIT](LICENSE-MIT))

Los binarios con CAD incluyen Open CASCADE enlazado de forma estática (LGPL-2.1 con la
excepción de Open CASCADE). El código fuente completo está disponible para volver a
enlazarlo.

Las contribuciones se aceptan bajo esas mismas dos licencias. Antes de contribuir, lee
[CONTRIBUTING.md](CONTRIBUTING.md), en especial las reglas de sala limpia.
