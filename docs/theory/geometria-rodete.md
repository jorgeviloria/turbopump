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

**Salida semiaxial.** Las dos paredes salen inclinadas $\varepsilon_s$ respecto a la normal
al eje, a $b_2$ de distancia. La carcasa termina en $(z_s, r_{2a})$ y el cubo en
$(L, r_{2i})$, sobre la pared del cubo:

$$\Delta = b_2\,(\cos\varepsilon_s, -\sin\varepsilon_s) + s\,(\sin\varepsilon_s, \cos\varepsilon_s),
\qquad s = \frac{r_{2i} - r_{2a} + b_2\sin\varepsilon_s}{\cos\varepsilon_s}$$

La esquina de cada Bézier es el punto donde la recta axial de entrada corta la dirección de
salida. Por defecto la carcasa conserva el tramo axial $(d_1 - d_n)/2$ del rodete radial y
luego sube con $\varepsilon_s$ hasta $d_{2a}$; $L$ suma el desfase $\Delta_z$ del cubo
(construcción propia). Con $\varepsilon_s = 0$ todo se reduce al rodete radial. Se rechaza
una $L$ tan corta que alguna esquina quede antes del plano de entrada.

**Paredes dibujadas por tramos.** Cubo y carcasa pueden dejar de ser una sola Bézier y
dibujarse como una cadena de tramos, cada uno Bézier o recta. El primero empieza en la
entrada y el último termina en la salida; esos dos puntos siguen saliendo de las
dimensiones principales y de $L$, así que la pared los acompaña cuando cambian. Las
uniones intermedias y los puntos de control interiores se guardan como $(z, r)$ absolutos.

Cada unión puede ser tangente o una esquina. En una unión tangente, el lado Bézier gira
hasta la tangente del otro lado y conserva la distancia de su punto de control a la unión;
entre dos Béziers, la segunda sigue a la primera, y dos rectas forman siempre esquina
(construcción propia). Las operaciones de edición son:

- **Dividir** un tramo en $t$ por de Casteljau: los dos tramos nuevos reproducen el
  original y quedan unidos con tangencia.
- **Unir** dos tramos contiguos: dos rectas dan una recta; en otro caso, una Bézier cúbica
  que conserva las tangentes de los extremos.
- **Cambiar el tipo:** una Bézier pasa a recta perdiendo sus puntos interiores, y una recta
  pasa a Bézier con los puntos en los tercios.
- **Mover una unión** arrastra también sus dos puntos de control vecinos, de modo que no
  pierde la tangencia.

Para el flujo cuasi-1D y los álabes la pared se muestrea con un número de puntos
proporcional a la longitud de cada tramo; una pared de un solo tramo da exactamente los
mismos puntos que antes. En la vista meridional, las uniones se dibujan como cuadrados.
Con el clic derecho sobre un punto se escriben sus coordenadas exactas; si la pared era
automática, pasa a dibujarse por tramos. Sobre la pared se puede dividir, cambiar el tipo
o volver al contorno automático.

**Borde de ataque:** se define por su posición en el cubo y en la carcasa, como fracción
de la longitud de cada curva. Por defecto está al 25 % en el cubo y al 5 % en la
carcasa. Entre esos dos puntos tiene una de tres formas:

- **Según el canal** (por defecto): en los spans intermedios se interpola linealmente la
  posición en la coordenada del canal.
- **Recta:** la recta geométrica del punto del cubo al de la carcasa.
- **Bézier cúbica:** sus dos puntos de control interiores se guardan como coordenadas
  $(u, v)$ a lo largo y a través de la cuerda cubo→carcasa, en fracciones de su longitud.
  Así la curva conserva su forma cuando sus extremos se deslizan por las paredes
  (construcción propia).

Con la recta o la Bézier, cada línea de corriente toma el borde de ataque donde corta la
curva; si no la corta, se vuelve a la interpolación según el canal. Al pasar a Bézier, la
curva nueva pasa por el borde actual en sus tercios, con
$P_1 = (-5P_0 + 18A - 9B + 2P_3)/6$ y $P_2 = (2P_0 - 9A + 18B - 5P_3)/6$, de modo que el
borde no salta.

**Borde de salida:** es la salida del canal: en $r_2$ en el rodete radial; en el
semiaxial, la recta de $d_{2i}$ a $d_{2a}$. En el semiaxial cada sección termina con su
$\beta_{2B}$ del vórtice libre ([dimensiones-principales.md](dimensiones-principales.md)).
Los álabes cilíndricos (2D) solo se admiten con salida radial.

**Editor del meridiano.** Es una vista propia para trabajar el meridiano con medidas:

- **Comprobación con Gülich** (cap. 7.2.2.1, Fig. 7.4 y ec. 7.2), punto por punto:
  - $z_E = (d_{2a} - d_1)(n_q/74)^{1{,}07}$, la extensión axial hasta el extremo de la
    carcasa; se da por buena dentro de ±25 % (tolerancia nuestra, porque Gülich admite
    valores menores en bombas pequeñas);
  - $R_{Ds} = (0{,}6 \text{ a } 0{,}8)\,b_1$, con $b_1 = (d_1 - d_n)/2$, frente al menor radio
    de curvatura de la carcasa;
  - $g_1 = (0{,}2 \text{ a } 0{,}3)\,b_1$, el tramo casi axial de la carcasa antes de
    curvarse; lo medimos hasta donde la carcasa ha girado 10° (criterio nuestro para el
    «aumento de radio pequeño» del libro);
  - el borde de ataque en la carcasa, antes de esa curva y no donde la curvatura es alta;
  - $\varepsilon_{Ds}$, el ángulo de salida de la carcasa: a menudo 0 por debajo de
    $n_q = 20$ y de unos 15–20° en rodetes radiales de $n_q$ mayor.
- **Herramientas de dibujo:** una rejilla a la que se ajustan los puntos arrastrados; la
  medida de distancias con Alt y arrastre; añadir un punto de control a un tramo Bézier
  (elevación de grado, sin cambiar la forma) o quitarlo; copiar los 100 puntos de una
  pared, y curvas de referencia a trazos, pegadas como texto o copiadas de una pared para
  comparar los cambios.

- **Cotas:** de $L$, de $b_2$ y de la $z$ y la $r$ de cada unión de las paredes dibujadas,
  más las del punto bajo el puntero. Las axiales se apilan bajo el eje y los radios a la
  derecha del rodete. Al hacer clic en el valor de una cota se escribe; la de $b_2$ solo
  se lee, porque viene de las dimensiones principales.
- **Líneas guía** en $d_n$, $d_1$ y $d_2$.
- **Curvatura máxima de cada pared**, con su radio $R = 1/\kappa_{máx}$. La curvatura de
  cada tramo Bézier sale de sus derivadas,
  $\kappa = |x'y'' - y'x''| / (x'^2 + y'^2)^{3/2}$; la de una recta es nula.
- **Círculos de área:** en algunas cuasi-ortogonales, un círculo de diámetro igual al
  ancho normal al flujo, $b_n = A / (\pi (r_h + r_s))$.
- **Tabla con todos los puntos** del cubo, la carcasa y el borde de ataque, editables.
- **Dos progresiones:** el área de paso $A/A_1$ a lo largo del canal y la curvatura
  $\kappa$ de cada pared a lo largo de su longitud. Son los criterios de comprobación de
  Gülich (cap. 7.2.2.1): la curvatura de las líneas de corriente exterior e interior y el
  área $A = 2\pi r b$ deben variar de forma continua a lo largo de la línea media.

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

**Flujo sobre el que se construyen los álabes.** Las cuasi-ortogonales conservan el
cálculo cuasi-1D (vistas y aviso de arriba). Las secciones del álabe, en cambio, se
colocan por defecto sobre las superficies de corriente del **flujo potencial 2D**
([prestaciones.md](prestaciones.md), sección 2), como hacen los programas comerciales con
su cálculo meridional. Se resuelve en las mismas 81 estaciones, así que el índice de un
punto sigue siendo su posición $t$ en el canal, y cada línea de corriente lleva su $c_m$
punto a punto. En el codo de axial a radial la cubierta es la pared interior: allí el
flujo va más rápido que la media y en el cubo más lento. En el rodete de referencia,
$c_m$ en el borde de ataque pasa de 6,5 a 8,5 m/s en la cubierta y de 5,9 a 4,1 m/s en el
cubo. Se puede volver al cálculo cuasi-1D; si el flujo potencial falla, se usa el
cuasi-1D y se avisa.

## 3. Propiedades del álabe

**Velocidades en el borde de ataque.** En cada span se toman el radio del borde de
ataque y la $c_m$ de su línea de corriente en ese punto (del flujo potencial, por
defecto), con entrada sin prerrotación:

$$u_1 = \omega r_{LE}, \qquad \beta_1 = \arctan(c_m/u_1), \qquad \beta_{1B} = \arctan(\tau_1 c_m/u_1) + i$$

Con el flujo potencial, en el rodete de referencia $\beta_{1B}$ pasa de 33° a 40° en la
cubierta y de 53° a 44° en el cubo: el álabe se tuerce menos en el borde de ataque. La
cubierta queda más corta (127 → 116 mm), y el criterio de carga de Gülich sube de 1,11 a
1,17, justo por encima de su banda.

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

**Envolvimiento común (por defecto).** Si cada sección envolviera lo que da su propia
distribución de β, el borde de ataque quedaría barrido en dirección tangencial. Sobre el
flujo potencial, en el rodete de referencia, el cubo envuelve 145° y la cubierta 97°. Eso
inclina el álabe hasta 36° respecto a la cubierta y alarga la nariz del redondeo. Como
hacen los programas comerciales con su "envolvimiento medio", todas las secciones toman el
mismo envolvimiento: la media de los que darían por sí solas, o uno fijado a mano. Cada
sección conserva $\beta_{1B}$, $\beta_{2B}$ y sus pendientes en los bordes; a su
distribución de β se le suma

$$\Delta\beta = a\,\bigl(4\bar m(1-\bar m)\bigr)^2$$

y $a$ se busca con el método de Brent para que $\theta_{LE}$ sea el común. El
envolvimiento baja al crecer $a$. $\beta$ se mantiene entre 5° y 90°; si así no se
alcanza el objetivo, se avisa. En la referencia todas las secciones envuelven 119,7°, el
álabe forma al menos 59° con las paredes y la carga de Gülich vuelve a 0,99. Con
"cada una el suyo" se recupera el comportamiento anterior.

**Comprobación de la integración.** Una línea radial con β constante debe dar la espiral
logarítmica $\theta = \ln(r_2/r_1)/\tan\beta$.

**Aviso.** Se avisa si el envolvimiento sale de 45°–200°.

## 5. Espesores y bordes

**Espesor normal.** Varía linealmente entre $e_{LE}$ y $e_{TE}$. Por defecto, ambos valen
el espesor de las dimensiones principales.

**Caras del álabe.** El espesor se aplica perpendicular a la línea media sobre la
superficie de corriente, como recomienda la práctica de los programas comerciales. Se
trabaja en el plano conforme $(\bar m, \theta)$, con $\bar m = \int dm/r$, que conserva
los ángulos y escala las longitudes por $1/r$:

$$\bar m_{ps/ss} = \bar m \pm \frac{e}{2r}\cos\beta, \qquad
\theta_{ps/ss} = \theta \pm \frac{e}{2r}\sin\beta$$

Lejos de los bordes da las mismas caras que aplicarlo en dirección circunferencial,
$\theta \pm (e/2)/(r\sin\beta)$, pero cada cara queda en su propia posición meridional.
Junto a un borde redondeado la diferencia importa: con el desplazamiento circunferencial
la nariz salía torcida, y una cara asomaba hasta un 15-20 % del espesor por delante del
borde. Ahora la nariz es simétrica respecto a la línea media y el $c_{p,min}$ de la
ec. 7.1a corresponde a la pieza.

Un borde cortado se corta sobre el borde meridional: cada cara se recorta, o se prolonga
por su último tramo, hasta $\bar m$ del borde. Solo se reajusta el cuarto de los puntos
junto a ese borde; el resto conserva un punto de cada cara frente a cada punto de la
línea media.

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

- **Longitud axial:** 46,4 mm hasta el cubo. La $z_E$ de la ecuación 7.2 (46 mm) se mide,
  según la Fig. 7.4 de Gülich, desde el plano de entrada hasta el extremo de la carcasa en
  $d_2$. Con nuestra $L$ por defecto la carcasa termina en $L - b_2 = 29$ mm, un 37 % más
  corta de lo recomendado. El editor del meridiano lo señala.
- **Velocidad meridional:** $c_m$ baja de 6,53 a 3,04 m/s sin repuntes.
- **Carga del álabe:** $\xi_{ef}/\xi_{adm} = 1{,}21/1{,}09 = 1{,}11$, dentro de la banda
  pero algo por encima del 0,9 recomendado. Con 5 álabes la carga sale de la banda y el
  programa avisa.
- **Borde de ataque:** $c_{p,min} = 0{,}41$.

## Limitaciones

- $c_m$ es uniforme en cada cuasi-ortogonal: falta la corrección por curvatura.
- No hay álabes partidos (splitters) ni álabes de superficie reglada. Los álabes 2D son
  cilíndricos (paralelos al eje); faltan los 2D de los rodetes axiales.
- El borde de ataque es una sola curva (recta o Bézier cúbica): no se divide en tramos.
- Las paredes por tramos solo admiten Béziers y rectas; faltan los arcos con rectas
  tangentes, las B-splines y la continuidad de curvatura en las uniones.
- No se modelan holguras ni redondeos.
- La exportación a TurboGrid pasa a la Fase 8 (CFD), donde se podrá validar contra el
  programa.
