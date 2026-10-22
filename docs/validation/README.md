# Validación: comparación con casos de referencia

Esta nota resume cómo se compara Turbopump con diseños de referencia, qué se ha
comparado, qué se añadió al programa a raíz de esas comparaciones, y qué limitaciones y
preguntas siguen abiertas.

> **Qué significa «validado» aquí.** Nada de lo que sigue certifica conformidad con
> ISO 9906, ISO 5199, API 610 ni ninguna otra norma. Esas normas fijan tolerancias para
> bombas **ensayadas**. Turbopump hace un diseño preliminar, y sus resultados solo se
> pueden **comparar** con otras predicciones o con datos publicados: «coincide dentro de
> ±x», «fuera de tolerancia» o «no reproducido».

## Cómo se compara

- **El comando.** `turbopump validate caso.tpj targets.json` calcula cada magnitud del
  archivo de objetivos y la juzga contra su tolerancia:
  - dentro, fuera, informativa (sin tolerancia) o no calculada;
  - termina con código 2 si algo con tolerancia o banda queda fuera;
  - `turbopump validate --list` da las 86 magnitudes disponibles.
- **Dónde está todo.** El formato y las reglas están en
  [tests/benchmarks/README.md](../../tests/benchmarks/README.md). La prueba
  [`crates/tp-project/tests/benchmarks.rs`](../../crates/tp-project/tests/benchmarks.rs)
  ejecuta cada caso.
- **Reglas:**
  - las tolerancias se fijan antes de calcular y se justifican en el archivo;
  - un valor que no cabe se documenta, y no se cambia ningún coeficiente para un caso;
  - una tolerancia solo se quita si la magnitud de la referencia resulta no ser la misma
    (otra estación, otra base de caudal), y se explica.
- **Tolerancias por defecto:**
  - geometría principal: ±1 % en d₂ y ±3 % en b₂, d₁ y los radios;
  - triángulos: ±3 % en u₂, c_u2, c₂ y w₂, y ±5 % en c_m;
  - ángulos: ±1°;
  - potencias: ±3 %;
  - rendimientos: ±2 puntos;
  - NPSH₃: dentro de la banda de los métodos publicados.

## Caso público: ejemplo de diseño de Japikse (cap. 7.5)

Japikse, Marscher y Furst, *Centrifugal Pump Design and Performance*, 1997, cap. 7.5,
Fig. 7.73. Es una bomba de 5000 gpm, 84 ft y 1000 rpm, con n_q ≈ 49, 7 álabes, difusor
sin álabes y voluta. Las entradas y la lectura completa están en
[tests/benchmarks/japikse-7.5/](../../tests/benchmarks/japikse-7.5/).

| Magnitud | Libro | Turbopump | Δ | Tolerancia | |
|---|---|---|---|---|---|
| n_q | 49,29 | 49,32 | +0,1 % | ±1 % | dentro |
| c_m2 | 9,222 ft/s | 9,460 ft/s | +2,6 % | ±5 % | dentro |
| β₂ | 14,0° | 13,5° | −0,5° | ±1° | dentro |
| β₂B | 22,5° | 21,75° | −0,75° | ±1° | dentro |
| λ a la entrada de la voluta | 4,18 | 4,11 | −1,7 % | ±5 % | dentro |
| Coeficiente de pérdidas de la espiral | 0,054 | 0,056 | +0,002 | ±0,003 | dentro |
| c_u2 | 39,89 ft/s | 37,44 ft/s | −6,1 % | ±3 % | **fuera** |
| α₂ | 13,0° | 14,2° | +1,2° | ±1° | **fuera** |
| η de la etapa | 81,8 % | 89,1 % | +7,3 pt | ±2 pt | **fuera** |
| Potencia | 130,3 hp | 118,4 hp | −9,1 % | ±3 % | **fuera** |

Hay además cinco magnitudes informativas: el deslizamiento, la entrada (c_m1, w₁ y β₁
en la carcasa, porque el libro la toma sobre un borde de ataque inclinado) y el NPSH del
libro, que no es NPSH₃.

**Lectura.**
- Lo que queda fuera se debe al rendimiento. Turbopump toma la estadística de Gülich
  (tabla 3.9), que da 89 % con n_q = 49 y 0,315 m³/s. El libro suma pérdidas, entre
  ellas una potencia de recirculación que fija, a propósito conservadora, en el 2 % de la
  de Euler.
- c_u2 y α₂ siguen del η_h.
- El ángulo de álabe que hace falta para la altura coincide dentro de 0,75°.
- La voluta coincide cuando se usan el difusor y el ancho de entrada del libro.

## Lo que aprendimos de un caso de un programa comercial

También se comparó un diseño de un programa comercial de diseño de turbomáquinas. Sus
datos no se publican y el caso vive fuera del repositorio. Las lecciones generales son
estas:

- **Primero, las definiciones.** Varias diferencias aparentes eran definiciones
  distintas, no física:
  - c_m calculada con el caudal de la bomba Q o con el del rodete Q_La = Q/η_v;
  - el radio «medio» de la entrada, aritmético o cuadrático;
  - la presión de entrada, 100 kPa o 101,325 kPa;
  - la velocidad en la garganta de una voluta doble, por voluta parcial o con el área
    total;
  - el rendimiento mecánico, con o sin el rozamiento de disco.

  `validate` ofrece las dos versiones de cada una (por ejemplo `cm2` y `cm2_q`).
- **Dimensiones principales.** Con la misma base, el diseño 1D coincide dentro de
  0,5–1,5 %:
  - b₂ o β₂B, triángulo de salida, η_h, η_v y η;
  - potencia, par y eje;
  - NPSH disponible.
- **NPSH₃.** Los métodos publicados con la misma fórmula y el mismo rango coinciden
  dentro de 0,5 %. La dispersión entre métodos es grande, así que el programa da una
  banda.
- **β₁B en el cubo.** Depende sobre todo del modelo de flujo meridional en el borde de
  ataque. Con el flujo potencial, un borde de ataque en el codo cóncavo del cubo ve una
  c_m muy baja, y lo mismo en mallas cada vez más finas. Con la c₁m uniforme de Gülich
  (ec. T7.1.7), el ángulo sube pero sigue por debajo de la referencia. No se ajustó nada.
- **Voluta.** Ninguna regla publicada (momento angular, Pfleiderer, Stepanoff) da las
  gargantas de la referencia. Según la ec. T4.1.3 de Gülich, la de Turbopump pone el
  punto óptimo de ese rodete en su caudal de diseño, y la de la referencia lo llevaría a
  más caudal.

## Lo que se añadió a raíz de la validación

| Función | Dónde | Ecuaciones y fuentes |
|---|---|---|
| `turbopump validate` y el formato `targets.json` | `tp-project::validation`, `tp-cli` | — |
| Triángulos con el caudal de la bomba Q | `CentrifugalPumpDesign::at_pump_flow` | Q = η_v·Q_La |
| Desglose η = η_h·η_v·η_RR·η_m y η_i | `performance::EfficiencyBreakdown` | Gülich tabla 3.5, ecs. T3.5.1, T3.5.4–T3.5.6 y T3.5.9; η_RR = 1 − ΣP_RR/P_i es propio |
| Borde de ataque por puntos (`le-points`) | `MeridionalInput::set_leading_edge_points` | ajuste cúbico con corrección de parámetros, Piegl y Tiller §9.4.1 |
| c₁m uniforme en el borde de ataque (`inlet-velocity=uniform`) | `InletVelocity` | Gülich tabla 7.1, ec. T7.1.7 |
| Envolvimiento lineal de cubo a carcasa (`wrap=linear:…`) | `WrapMode::Linear` | construcción propia; rangos de Gülich cap. 7.2.2.2 |
| Banda de NPSH₃ por cinco métodos | `npsh::npsh3_band` | ver abajo |
| Compensación del nervio de la voluta doble (`rib_comp`) | `VoluteSetupInput::rib_compensation` | construcción propia, como la del corte de agua |
| Punto óptimo según la garganta del colector | `performance::throat_best_efficiency_flow` | Gülich tabla 4.1, ec. T4.1.3 |

Métodos de NPSH₃:
- coeficientes λ_c y λ_w: Gülich ec. 6.10, cap. 6.3.2 y tabla D6.3;
- n_ss según el tipo de rodete: Gülich ec. 6.7 y tabla D6.1;
- Stepanoff, S = 7900: 2.ª ed., ecs. 12.16 y 12.18;
- Petermann, S_q 0,4–0,45: Pfleiderer y Petermann, *Strömungsmaschinen*;
- Europump, (0,3–0,5)·n√Q: *NPSH for Rotodynamic Pumps*, 1999.

La teoría de cada una está en [docs/theory/](../theory/).

## Limitaciones

- **Borde de ataque.** El ángulo de entrada depende del modelo de c_m: el flujo
  potencial o el cuasi-1D a lo largo de la línea de corriente, o la c₁m uniforme del ojo.
  No hay corrección por curvatura en el cuasi-1D, ni capa límite en el potencial.
- **Spans.** Los spans reparten el caudal por igual. Las referencias que reparten la
  longitud del borde de ataque dan otros spans intermedios, que no son comparables.
- **Meridiano:**
  - las paredes son Béziers y rectas, sin tramos B-spline;
  - el plano de entrada es normal al eje, sin inclinación;
  - un meridiano importado por puntos entra como cadena de rectas.
- **Voluta:**
  - la regla de Pfleiderer con exponente x ≠ 1 no está comprobada contra su libro;
  - el desfase angular del nervio de una voluta doble no se modela, porque no hay una
    definición publicada;
  - la voluta doble no admite reglas geométricas.
- **Pérdidas:**
  - los valores absolutos de la tabla 3.8 de Gülich son imprecisos, como avisa el propio
    libro;
  - con n_q bajo, el rozamiento en las paredes de la voluta (ec. T3.8.21) es una parte
    grande de H_th, y la suma de pérdidas queda unos puntos por debajo de la
    estadística.
- **Rendimiento.** La estadística de Gülich (tabla 3.9) es la base del diseño. Su
  dispersión (±0,2·(1 − η)) es real, como se ve en el caso de Japikse.
- **NPSH₃.** El método de Lobanoff y Ross no se implementa: da un NPSHR sin cavitación,
  con coeficientes que solo existen en gráficas.

## Lo que queda abierto

1. **β₁B en el borde de ataque.** Se podría probar un equilibrio radial con curvatura de
   líneas de corriente en las cuasi-ortogonales, como término medio entre el cuasi-1D y
   el potencial.
2. **Meridiano.** Tramos B-spline e importación de contornos por puntos sin pasar por
   rectas.
3. **Voluta doble.** Reglas geométricas (A/r_c final) y, si se acuerda una definición
   propia, un desfase radial del nervio.
4. **Rendimiento.** Contrastar la suma de pérdidas con más casos publicados de n_q bajo
   antes de usarla en lugar de la estadística.
5. **Más casos públicos:** la bomba de ERCOFTAC y ensayos publicados con curva.
6. **Fase 4: tablas y gráficos propios.** Correlaciones editables, que el usuario
   digitaliza de la literatura, y gráficos de resultados, en la GUI y en archivo.
7. **Rodete abierto y rendimiento del motor.** Solo si hacen falta.
