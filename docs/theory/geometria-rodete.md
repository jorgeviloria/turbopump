# Geometría del rodete radial

Implementación: [`crates/tp-design/src/impeller/`](../../crates/tp-design/src/impeller/)

La geometría parte de las dimensiones principales: $d_n$, $d_1$, $d_2$, $b_2$, $z$, $e$,
$\beta_{2B}$ y $Q_{La}$ ([dimensiones-principales.md](dimensiones-principales.md)). La
construyen cinco pasos de diseño, en este orden:

1. contorno meridional;
2. propiedades del álabe;
3. líneas medias;
4. perfiles (espesores);
5. bordes.

En un rodete de doble aspiración se modela una mitad: la mitad del caudal y la mitad de
$b_2$.

| Vista meridional | Vista frontal desde la entrada |
|---|---|
| ![meridional](../img/rodete-referencia-meridional.svg) | ![frontal](../img/rodete-referencia-frontal.svg) |

*Rodete de referencia: 100 m³/h, 32 m, 2900 rpm, todos los valores automáticos.*

## Coordenadas

- $z$: axial, en el sentido del flujo de entrada. El plano de entrada es $z = 0$.
- $r$: radio.
- $\theta$: posición angular de la línea media, medida **desde el borde de salida** en el
  sentido de giro.

**Paso a coordenadas cartesianas:**

$$x = r\cos(s\,\theta), \qquad y = r\sin(s\,\theta), \qquad z = z$$

Aquí $s = +1$ si el rodete gira en sentido positivo alrededor de $+z$, que es el sentido
horario visto desde la entrada mirando aguas abajo, y $s = -1$ si gira al revés.

## 1. Contorno meridional

Cubo y carcasa son Béziers cúbicas que salen axiales del plano de entrada y llegan
radiales a $r_2 = d_2/2$:

| Curva | Punto de inicio | Esquina de las tangentes | Punto final |
|---|---|---|---|
| Cubo | $(0, r_n)$ | $(L, r_n)$ | $(L, r_2)$ |
| Carcasa | $(0, r_1)$ | $(L - b_2, r_1)$ | $(L - b_2, r_2)$ |

Los puntos de control interiores están a una fracción $t$ de la distancia entre cada
extremo y la esquina. El valor por defecto es 0,55, cercano al 0,552 con el que una
Bézier cúbica reproduce un cuarto de elipse.

**Longitud axial por defecto:** $L = b_2 + (d_1 - d_n)/2$. Es un valor por defecto de
ingeniería, no una correlación publicada.

**Borde de ataque:** se define por su posición en el cubo y en la carcasa, como fracción
de la longitud de cada curva. Por defecto está al 25 % en el cubo y al 5 % en la
carcasa. En los spans intermedios se interpola linealmente en la coordenada del canal.

**Borde de salida:** está en $r_2$, la salida del canal.

## 2. Flujo meridional (cuasi-1D)

**Cuasi-ortogonales.** Son segmentos rectos que unen puntos del cubo y de la carcasa
situados a la misma fracción $t$ de su longitud. Se usan 81.

**Área de paso.** Cada segmento barre al girar un tronco de cono de área
$\pi (r_h + r_s)\,\ell$. Se corrige con el seno del ángulo entre el segmento y la línea de
corriente media:

$$A = \pi (r_h + r_s)\,\ell\,\sin\alpha, \qquad c_m = Q/A$$

Así, el área es exacta en la entrada, $\pi(r_1^2 - r_n^2)$, y en la salida, $2\pi r_2 b_2$.

**Líneas de corriente (spans).** Reparten el caudal por igual con $c_m$ uniforme en cada
cuasi-ortogonal. El punto a fracción $\lambda$ del segmento que encierra la fracción $f$
del área cumple:

$$\Delta r\,\lambda^2 + 2 r_h\,\lambda - f\,(2 r_h + \Delta r) = 0$$

Se resuelve sin cancelación numérica.

**Aviso.** Si $c_m$ sube más de un 5 % respecto a su mínimo aguas arriba, el contorno
acelera el flujo localmente y conviene suavizarlo.

> El método potencial 2D (función de corriente) que corrige $c_m$ por la curvatura del
> canal queda para la Fase 7.

## 3. Propiedades del álabe

**Velocidades en el borde de ataque.** En cada span se toman el radio y la $c_m$ reales
del borde de ataque, con entrada sin prerrotación:

$$u_1 = \omega r_{LE}, \qquad \beta_1 = \arctan(c_m/u_1), \qquad \beta_{1B} = \arctan(\tau_1 c_m/u_1) + i$$

**Ángulo de salida.** $\beta_{2B}$ es el mismo en todos los spans, porque la salida es
radial con $u_2$ y $c_{m2}$ uniformes.

**Número de spans.** Por defecto son 5.

**Forma del álabe.** Por defecto el álabe es **libre (3D)**: cada span tiene su propio
$\beta_{1B}$. Las formas **cilíndricas (2D)** extruyen una sola planta $\theta(r)$ a lo
largo del eje, así que todas las secciones la comparten. El álabe no tiene inclinación y
se funde con un macho de una pieza. A cambio, su ángulo de entrada solo coincide con el
flujo cerca de un span. Lobanoff y Ross (cap. 3) lo aceptan para $N_s$ = 500-1000
($n_q$ ≈ 10-19). Gülich trata estos rodetes en el cap. 7.3.

La planta pasa por el borde de ataque del span central y por $(r_2, \beta_{2B})$:

- **cilíndrico:** $\beta(r)$ sigue la distribución de las líneas medias, con
  $\bar r = (r - r_1)/(r_2 - r_1)$;
- **arco circular** (ec. 7.10 reordenada):
  $\cos\beta(r) = \left[r_2\cos\beta_2 + k\,(r^2 - r_2^2)\right]/r$, con
  $k = (r_2\cos\beta_2 - r_1\cos\beta_1)/(r_2^2 - r_1^2)$;
- **recto:** $k = 0$, es decir $r\cos\beta = r_2\cos\beta_{2B}$. Con
  $\beta_{2B} = 90°$ los álabes son radiales y no envuelven (cap. 7.3.3).

$\theta(r) = \int_r^{r_2} dr/(r\tan\beta)$ se integra una sola vez. En cada sección,
$\theta$ se toma en el radio local, y el ángulo sobre la superficie de corriente cumple
$\cot\beta = (dr/dm)\cot\beta_{\text{planta}}$: vale 90° donde la sección es axial. Por
eso el ángulo de la planta en el span central es $\arctan(\tan\beta_{1B}\,dr/dm)$, y se exige
$dr/dm \ge 0{,}2$ allí.

Con forma cilíndrica y el borde de ataque en automático, el borde se coloca paralelo al
eje: en el radio donde la cubierta ya giró 30° respecto a la dirección axial, y con el punto
del cubo a ese mismo radio. Se avisa si algún span recibe el flujo a más de 5° de su
ángulo de entrada; esa diferencia es la incidencia que el álabe 2D impone fuera del span
central.

## 4. Líneas medias (mapeo conforme)

**Distribución de β.** $\beta$ se define sobre la longitud meridional normalizada
$\bar m \in [0, 1]$, entre $\beta_{1B}$ y $\beta_{2B}$:

- **lineal**, la opción por defecto;
- **suave**: una Bézier en $(\bar m, \beta)$ que sale y llega con pendiente nula. Los
  parámetros `inlet` y `outlet` fijan cuánto se mantiene β cerca de sus valores
  extremos.

**Integración.** Con $\tan\beta = dm/(r\,d\theta)$:

$$\theta(m) = \int_m^{m_{TE}} \frac{dm}{r \tan\beta}$$

La integral se calcula con trapecios sobre 2000 pasos.

**Apilado.** Todas las secciones tienen $\theta = 0$ en la salida, así que el borde de
salida es una recta axial. El ángulo de envolvimiento es $\theta$ en el borde de ataque.

**Comprobación de la integración.** Una línea radial con β constante debe dar la espiral
logarítmica $\theta = \ln(r_2/r_1)/\tan\beta$.

**Aviso.** Se avisa si el envolvimiento sale de 45°–200°.

## 5. Espesores y bordes

**Espesor normal.** Varía linealmente entre $e_{LE}$ y $e_{TE}$. Por defecto, ambos valen
el espesor de las dimensiones principales.

**Caras del álabe.** Se aplica el espesor en dirección circunferencial sobre la superficie
de corriente:

$$\theta_{ps/ss} = \theta \pm \frac{e/2}{r \sin\beta}$$

Esto da exactamente el espesor normal $e$ y mantiene ambas caras sobre el mismo span.

- La **cara de presión** es la que mira en el sentido de giro.
- **Interferencia.** Se comprueba que el espesor circunferencial no llegue al paso
  $2\pi/z$.

**Bordes.**

| Tipo | Forma | Uso por defecto |
|---|---|---|
| Elipse de relación $k$ | longitud de la nariz $L_p = k\,e/2$ sobre la línea media; espesor $\propto \sqrt{1 - ((a-s)/a)^2}$ | borde de ataque, $k = 4$ |
| Cortado | espesor completo hasta el borde | borde de salida |

**Borde de ataque.** Gülich valora el perfil por su coeficiente de presión mínima con
entrada sin choque (ec. 7.1a):

$$c_{p,min} = 2x + x^2, \qquad x = 0{,}373\,e/L_p$$

| Perfil | $c_{p,min}$ |
|---|---|
| Semicírculo, $k = 1$ | 2,05 (desfavorable) |
| Elipse 2:1 | 0,89 |
| Elipse 4:1 (por defecto) | 0,41 |
| Elipse 10:1 | 0,155 |

Las elipses largas, en forma de cuña, solo funcionan bien con incidencia nula y son
frágiles en fundición. Por eso el valor por defecto se queda en 4:1; es una elección
propia, y el informe muestra el $c_{p,min}$ resultante.

**Borde de salida.** El espesor completo hasta el borde deja material para limarlo por
debajo si falta altura (pp. 348-349).

## 6. Carga del álabe (tabla 7.1, ecs. 7.1.10-7.1.11)

$$\xi_{ef} = \frac{2\pi\,\psi_{opt}}{\eta_h\,z\,L^*\,(w_1^* + w_2^*)}, \qquad \xi_{adm} = \left(\frac{40}{n_q}\right)^{0,77}$$

- Las magnitudes con asterisco se refieren a $d_2$ (longitudes) y a $u_2$ (velocidades).
- Según la convención del libro, las magnitudes sin subíndice son de la línea de
  corriente exterior. Por eso $L$ (longitud del álabe) y $w_1$ se toman en la sección de
  la cubierta.
- El número y la longitud de los álabes son adecuados si $\xi_{ef}$ queda en la banda de
  ±15 % alrededor de $\xi_{adm}$. Gülich recomienda quedarse un 10 % por debajo.
- Se avisa si el álabe está poco cargado (sobran álabes o longitud) o sobrecargado
  (faltan).

## 7. Salidas

`turbopump geometry proyecto.tpj --export carpeta` escribe los archivos de la tabla.
`--dwg-version` elige el formato de los DWG (2018 por defecto).

| Archivo | Contenido |
|---|---|
| `impeller.txt` | Cubo, carcasa y cada sección: m, z, r, θ, β, espesor y puntos x y z de la línea media y las dos caras (SI, 12 cifras) |
| `impeller.stl` | Malla de todos los álabes, cubo y carcasa en mm, para verla en FreeCAD, ParaView u otro visor. No es un sólido cerrado |
| `meridional.svg` | Contorno, spans, cuasi-ortogonales y bordes |
| `front.svg` | Vista desde la entrada con las secciones de cubo y carcasa |
| `meridional.dxf`, `front.dxf` | Los mismos dibujos en DXF (AutoCAD 2000, mm), con una capa por tipo de línea |
| `meridional.dwg`, `front.dwg` | Los mismos dibujos en DWG, escritos por Turbopump con acadrust |

Los dos dibujos salen de un modelo de plano común (`tp_io::drawing`). El cubo y la
carcasa conservan su forma exacta: son las curvas de Bézier del diseño, que en SVG se
escriben como trazos cúbicos y en DXF y DWG como `SPLINE` de grado p con p + 1 polos y los
nudos 0 y 1 repetidos p + 1 veces (Piegl y Tiller, *The NURBS Book*, 2.ª ed., §2.2).
Las curvas que el diseño calcula punto a punto (spans, secciones del álabe) son
polilíneas.

| Capa | Contenido |
|---|---|
| `CONTOUR` | Cubo, carcasa y diámetro exterior |
| `AXIS` | Eje (línea de trazo y punto `CENTER`) |
| `CONSTRUCTION` | Cuasi-ortogonales y círculos de la boca y el cubo |
| `STREAMLINES` | Spans |
| `BLADE-EDGES` | Bordes de ataque y de salida, con sus rótulos |
| `BLADE-HUB`, `BLADE-SHROUD` | Contorno del álabe en el cubo y en la carcasa (vista frontal) |
| `TEXT` | Pie del dibujo con las dimensiones principales |

## 8. Resultados de referencia

Valores automáticos, con $d_1 = 77{,}2$ mm por el factor $f_{d1}$ de la tabla 7.1:

| Span | $r_{LE}$ | $c_m$ | $\beta_1$ | $\beta_{1B}$ | Envolvimiento | Longitud |
|---|---|---|---|---|---|---|
| cubo | 18,9 mm | 5,89 m/s | 45,7° | 52,8° | 120,7° | 128,0 mm |
| medio | 30,4 mm | 6,27 m/s | 34,2° | 39,5° | 120,5° | 130,8 mm |
| carcasa | 38,9 mm | 6,50 m/s | 28,8° | 33,2° | 111,6° | 126,7 mm |

- **Longitud axial:** 46,4 mm. Coincide con la estimación $z_E$ de la ecuación 7.2 del
  libro (46 mm), aunque falta confirmar desde dónde se mide esa cota.
- **Velocidad meridional:** $c_m$ baja de 6,53 a 3,04 m/s sin repuntes.
- **Carga del álabe:** $\xi_{ef}/\xi_{adm} = 1{,}21/1{,}09 = 1{,}11$, dentro de la banda
  pero algo por encima del 0,9 recomendado. Con 5 álabes la carga sale de la banda y el
  programa avisa.
- **Borde de ataque:** $c_{p,min} = 0{,}41$.

## Limitaciones

- $c_m$ es uniforme en cada cuasi-ortogonal: falta la corrección por curvatura.
- No hay álabes partidos (splitters) ni álabes de superficie reglada. Los álabes 2D son
  cilíndricos (paralelos al eje); faltan los 2D de los rodetes axiales.
- El borde de ataque es recto en la coordenada del canal.
- No se modelan holguras ni redondeos.
- La exportación a TurboGrid pasa a la Fase 8 (CFD), donde se podrá validar contra el
  programa.
