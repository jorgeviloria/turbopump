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
| Elipse de relación $k$ | longitud de la nariz $k\,e/2$ sobre la línea media; espesor $\propto \sqrt{1 - ((a-s)/a)^2}$ | borde de ataque, $k = 2$ |
| Cortado | espesor completo hasta el borde | borde de salida |

Es la forma habitual de los rodetes de bomba fundidos.

## 6. Salidas

`turbopump geometry proyecto.tpj --export carpeta` escribe:

| Archivo | Contenido |
|---|---|
| `impeller.txt` | Cubo, carcasa y cada sección: m, z, r, θ, β, espesor y puntos x y z de la línea media y las dos caras (SI, 12 cifras) |
| `impeller.stl` | Malla de todos los álabes, cubo y carcasa en mm, para verla en FreeCAD, ParaView u otro visor. No es un sólido cerrado |
| `meridional.svg` | Contorno, spans, cuasi-ortogonales y bordes |
| `front.svg` | Vista desde la entrada con las secciones de cubo y carcasa |

## 7. Resultados de referencia

| Span | $r_{LE}$ | $c_m$ | $\beta_1$ | $\beta_{1B}$ | Envolvimiento |
|---|---|---|---|---|---|
| cubo | 19,3 mm | 6,58 m/s | 48,4° | 55,0° | 112,8° |
| medio | 29,1 mm | 7,14 m/s | 38,9° | 44,1° | 110,4° |
| carcasa | 36,5 mm | 7,45 m/s | 33,9° | 38,3° | 103,2° |

- $c_m$ baja de 7,49 a 3,04 m/s sin repuntes. Coincide con la $c_{m1}$ y la $c_{m2}$ de
  las dimensiones principales.
- El envolvimiento entre 100° y 115° es el rango habitual para $n_q \approx 36$.

## Limitaciones

- $c_m$ es uniforme en cada cuasi-ortogonal: falta la corrección por curvatura.
- No hay álabes partidos (splitters), álabes de superficie reglada ni álabes 2D de
  elemento radial.
- El borde de ataque es recto en la coordenada del canal.
- No se modelan holguras ni redondeos.
- La exportación a TurboGrid pasa a la Fase 8 (CFD), donde se podrá validar contra el
  programa.
