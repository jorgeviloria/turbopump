# Voluta y difusores

Implementación:

- [`crates/tp-design/src/volute/`](../../crates/tp-design/src/volute/)
- [`crates/tp-design/src/diffuser/`](../../crates/tp-design/src/diffuser/)
- [`crates/tp-design/src/outlet.rs`](../../crates/tp-design/src/outlet.rs)

El rodete entrega el fluido con mucha velocidad tangencial: en la bomba de referencia,
$c_{2u} = 13{,}2$ m/s frente a $c_{2m} = 2{,}8$ m/s. Los componentes que siguen convierten
parte de esa energía cinética en presión y llevan el caudal hasta la tubería. Turbopump
tiene tres:

- **voluta de salida:** canal en espiral, lengüeta y difusor de descarga;
- **difusor sin álabes:** anillo de paredes paralelas;
- **difusor con álabes:** corona de álabes fijos entre $d_3$ y $d_4$.

Las referencias son de Gülich, *Centrifugal Pumps*, 2.ª edición: capítulos 7.8 (voluta),
7.9 y 7.12 (difusores), tabla 7.7 (procedimiento de cálculo) y tabla 10.2 (holguras y
número de álabes contra las pulsaciones de presión). Aquí solo se citan los números de
ecuación; la redacción y los valores por defecto son propios.

![Etapa con difusor](../img/etapa-difusor-frontal.jpg)

*Vista frontal desde la entrada: rodete de 7 álabes, difusor de 12 álabes y voluta
dimensionada desde $d_4$. La bomba de referencia es de 100 m³/h, 32 m y 2900 rpm.*

## Acoplamiento entre componentes

Cada componente toma su entrada del que tiene delante. Esa entrada es la salida del
rodete o del estator anterior:

| Dato | Tras un rodete | Tras un estator |
|---|---|---|
| Caudal | $Q$ del rodete | el mismo |
| Diámetro | $d_2$ | $d_4$ |
| Anchura | $b_2$ | $b_4$ |
| Velocidad tangencial | $c_{2u}$ | $c_{4u}$ |
| Velocidad meridional | $c_{2m}$ | $c_{4m}$ |

También pasan la velocidad específica $n_q$, la altura de la etapa, la densidad, el plano
medio del canal, el número de álabes del rodete y el sentido de giro.

Si cambia un dato aguas arriba, los pasos de los componentes de detrás quedan
desactualizados (⟲) hasta que se vuelven a aceptar.

**Ángulos:** el ángulo de la espiral $\varepsilon$ se mide desde la lengüeta en el sentido
de giro. La lengüeta está sobre el eje $+x$, y una sección de ángulo $\varepsilon$ queda en
$\theta = s\,\varepsilon$. El signo $s$ es el mismo que en el rodete
([geometria-rodete.md](geometria-rodete.md)).

## Voluta

Se diseña en seis pasos: datos de partida, sección transversal, desarrollo de la
espiral, difusor de descarga, lengüeta y espesor de pared.

### 1. Datos de partida

- **Caudal de diseño:** $Q_{Le} = Q \cdot (Q_{Le}/Q)$. El factor vale 1 por defecto y
  permite reservar caudal para las fugas que pasan por la voluta (ec. T7.7.1).
- **Diámetro de la lengüeta:** el mínimo de la tabla 10.2 para que las pulsaciones de
  presión sean bajas:

  $$\frac{d_z}{d_2} \ge 1{,}03 + 0{,}1\,\frac{n_q}{40} + 0{,}07\,\frac{\rho H}{\rho_{Ref} H_{Ref}}$$

  Aquí $\rho_{Ref} = 1000$ kg/m³ y $H_{Ref} = 1000$ m. El valor automático es ese
  mínimo, y un valor manual por debajo genera un aviso.
- **Espesor de la lengüeta:** $e_3 \approx 0{,}02\,d_2$ (cap. 7.8.1). El flujo se
  estanca en la nariz, a $r'_z = r_z + e_3/2$, y desde ese radio se integran las
  secciones.
- **Anchura de entrada:** $b_3/b_2$ baja linealmente de 2,0 (con $n_q \le 15$) a 1,2 (con
  $n_q \ge 55$). Queda dentro de los márgenes que da el libro para $n_q$ bajas y altas.
  La interpolación es una elección propia.

### 2. Sección transversal

La sección empieza en $r'_z$ con anchura $b_3$ y crece hacia fuera. Hay tres formas:

- **rectangular;**
- **trapezoidal**, con flancos abiertos 30° por defecto;
- **circular.**

Las esquinas de las secciones se redondean al dibujarlas. El contorno se ajusta
iterando hasta que su área coincide con la de diseño, porque si no el redondeo le
quitaría un 2 %.

### 3. Desarrollo de la espiral

**Conservación del momento angular** (ecs. 7.24 y 7.25). Sin rozamiento,
$c_u r = c_{2u} r_2$ en toda la voluta. El caudal que atraviesa la sección de ángulo
$\varepsilon$ es la fracción $\varepsilon/2\pi$ del total, y eso fija su altura:

$$\int_{r'_z}^{r_a} \frac{b(r)}{r}\,dr = \frac{\varepsilon\,Q_{Le}}{2\pi\,c_{2u}\,r_2}$$

Cada forma de sección se resuelve así:

- **rectangular:** $b_3 \ln(r_a/r'_z)$ igual al segundo miembro, que tiene solución
  cerrada (ec. T7.7.9);
- **circular:** el diámetro sale de $d = X + \sqrt{2\,d'_z X}$ (ec. T7.7.7);
- **trapezoidal:** la integral también es analítica, pero la altura se despeja con el
  método de Brent.

**Velocidad constante** (ecs. 7.26 y 7.27). El área de la garganta $A_{3q}$ sale del
momento angular, y el resto de secciones crece en proporción al ángulo:

$$A(\varepsilon) = A_{3q}\,\frac{\varepsilon}{2\pi}$$

**Ley automática:** velocidad constante por debajo de $n_q = 30$ y momento angular por
encima. El libro sitúa el cambio entre 25 y 35.

**Corrección por la lengüeta** (ec. 7.28): la lengüeta estorba al flujo en la garganta,
y su altura se aumenta en

$$\frac{\Delta a_3}{a_3} = 0{,}2\left(\frac{e_3}{a_3}\right)^2$$

**Secciones:** por defecto son 36. La espiral empieza donde su pared exterior deja de
chocar con la pared interior de la tobera; en la bomba de referencia, a 13°.

### 4. Difusor de descarga

Es un cono recto desde la garganta hasta un círculo de diámetro $d_d$. La pared del lado
de la lengüeta es la recta que pasa por la nariz, así que la tobera no estrecha el paso
junto a la espiral.

El radio equivalente de entrada es $R_1 = \sqrt{A_{3q}/\pi}$. Para no despegar, el
semiángulo de apertura no debe pasar del admisible (ec. 1.44):

$$\vartheta_{al} = 16{,}5°\sqrt{R_1/L}$$

El semiángulo equivalente de un difusor con relación de áreas $A_R$ es (ec. 1.45):

$$\tan\vartheta_{eq} = \frac{R_1}{L}\left(\sqrt{A_R} - 1\right)$$

Valores automáticos:

- longitud $L = 4\,R_1$;
- diámetro de salida para abrir al 85 % de $\vartheta_{al}$;
- por encima de $n_q = 80$ la tobera no decelera más ($A_R = 1$), y si se fuerza otra
  cosa el programa avisa.

La recuperación ideal es $c_{p,id} = 1 - 1/A_R^2$ (ec. 1.40).

### 5. Lengüeta

El ángulo del flujo en la lengüeta sale del vórtice libre y del caudal que pasa por el
círculo $d_z$ (ecs. T7.7.3 a T7.7.5). Con una sola lengüeta no hay bloqueo:

$$c_{3u} = c_{2u}\,\frac{d_2}{d_z}, \qquad c_{3m} = \frac{Q_{Le}}{\pi\,d_z\,b_3}, \qquad
\tan\alpha'_3 = \frac{c_{3m}}{c_{3u}}$$

El ángulo de la lengüeta es $\alpha_{3B} = \alpha'_3 + i_3$ (ec. T7.7.6). La incidencia
vale 0 por defecto, y fuera de ±3° el programa avisa.

### 6. Espesor de pared

Por defecto es $\max(0{,}03\,d_2,\ 6\ \text{mm})$, un valor propio. De momento solo se
guarda y se dibuja: el sólido de la carcasa llega en una fase posterior.

### 7. Voluta doble

Una voluta doble (Gülich, cap. 7.8.1 y Fig. 7.39) parte la espiral con un **nervio**
para que la presión alrededor del rodete se equilibre fuera del punto de diseño y baje
el empuje radial (cap. 9.3.4 y tabla 9.4). Se elige en el paso de datos de partida.

**Partes:**

- **Primera voluta parcial:** de la lengüeta a la nariz del nervio, en
  $\varepsilon_r = 2\pi - \varepsilon_{sp}$. Sus secciones son las de la voluta simple.
- **Voluta interior:** de la nariz del nervio a la garganta, en $2\pi$. Abarca
  $\varepsilon_{sp}$ y recoge su parte del caudal.
- **Canal exterior:** lleva el caudal de la primera voluta parcial por fuera del nervio
  hasta la descarga. Su área va de la garganta de la primera voluta parcial a esa área
  por la relación $A_{sal}/A_{ent}$, que por defecto es 1 (área constante, elección
  propia). Gülich dice que a $n_q$ bajo y medio el canal frena el flujo y se dimensiona
  como un difusor (cap. 1.6); el usuario puede fijar la relación.
- **Descarga:** empieza en la sección que abarca la voluta interior, el nervio y el
  canal exterior, y sigue como en la voluta simple con el área de los dos canales.

**Dimensiones:**

- Cada voluta parcial de ángulo $\varepsilon_{sp}$ se dimensiona para el caudal
  $Q_{Le}\,\varepsilon_{sp}/2\pi$ con la conservación del momento angular (cap. 7.8.1,
  punto 8). Su garganta cumple $\int b/r\,dr = \varepsilon_{sp}\,Q_{Le}/(2\pi c_{2u} r_2)$
  y lleva la corrección por la lengüeta (ec. 7.28).
- Con la ley de velocidad constante, el área crece en proporción al ángulo dentro de
  cada voluta parcial.
- Como $\int b/r\,dr$ crece con el logaritmo del radio, la garganta para la mitad del
  caudal es **menor que la mitad** de la garganta simple. Con secciones rectangulares es
  $1/(1 + \sqrt{1 + a/r'_z})$ veces la simple.

**Valores por defecto y límites:**

| Dato | Valor | Fuente |
|---|---|---|
| $\varepsilon_{sp}$ | 180°, o 170° con un número par de álabes, para que dos álabes no pasen a la vez por las dos lengüetas | Gülich cap. 7.8.1, punto 1 |
| Por debajo de 90° | error: no reduce el empuje radial | idem |
| Por encima de 180° | aviso: el canal exterior largo añade pérdidas y empuje en sobrecarga | idem |
| Espesor del nervio | $e_3$, el de la lengüeta (Gülich lo deja al cálculo resistente y a la fundición) | elección propia |

**En la predicción:**

- La garganta del colector del NPSH₃ es la media de las dos, con $z_{Le} = 2$.
- El modelo de pérdidas suma las dos gargantas: cada voluta parcial recoge su parte del
  caudal sobre su parte de la entrada, así que $AR$ es el mismo.
- El empuje radial usa $F_{Dsp}$ de la tabla 9.4 con el ángulo del nervio.

En la bomba de referencia (7 álabes, 180°):

| Magnitud | Simple | Doble |
|---|---|---|
| Garganta | 3123 mm² | 2 × 1408 mm² |
| $c_{3q}/c_2$ | 0,65 | 0,73 |
| $B/H$ | 1,67 | 2,04 |
| $F_R$ a válvula cerrada | 487 N | 125 N |
| $F_R$ en el diseño | 30 N | 102 N |
| Carga para el diseño mecánico | 640 N | 277 N |

La voluta doble tiene un empuje casi constante con el caudal (Fig. 9.29): en el diseño
es algo mayor que el de la simple, pero el máximo, y con él la carga del eje, baja a
menos de la mitad.

**CAD:** la primera voluta parcial con el canal exterior, la voluta interior y la
descarga se construyen como tres sólidos que se unen; los dos canales entran un poco en
la descarga para que la unión no trabaje con caras coincidentes. El resultado es un único
sólido válido.

### Comprobaciones

Estas dos cifras se informan sin aviso:

- **$c_{3q}/c_2$:** la tabla 10.2 aconseja no bajar mucho de 0,8. Con la ley de momento
  angular sale por debajo de forma natural (0,65 en la bomba de referencia).
- **$B/H$:** la relación ancho/alto de la garganta, de 2 a 3.

Generan aviso:

- $d_z$ por debajo del mínimo;
- una incidencia fuera de ±3°;
- una tobera más abierta que $\vartheta_{al}$;
- una tobera que decelera por encima de $n_q = 80$.

## Difusor sin álabes

Es un anillo entre $d_3$ y $d_4$:

- $d_3 = d_2$;
- $b_3 = 1{,}05\,b_2$ y $b_4 = b_3$;
- $d_4 = 1{,}5\,d_2$.

Estos valores son propios.

Se supone vórtice libre sin rozamiento: $c_u r = c_{2u} r_2$ y
$c_m = Q_{Le}/(2\pi r b)$. Con anchura constante, las dos componentes caen como $1/r$.
El ángulo $\alpha$ queda constante y las partículas siguen una espiral logarítmica:

$$\Delta\theta = \frac{\ln(r_4/r_3)}{\tan\alpha}, \qquad
\Delta s = \frac{r_4 - r_3}{\sin\alpha}$$

Esa longitud de recorrido es la que gasta el rozamiento en un anillo real. La
recuperación sin pérdidas es $c_{p,id} = 1 - (c_4/c_3)^2$.

## Difusor con álabes

Se diseña en los mismos pasos que un rodete: dimensiones principales, contorno
meridional, propiedades de los álabes, líneas medias, perfiles y bordes.

### Dimensiones principales y contorno

- **Diámetro de entrada:** el mínimo de la tabla 10.2 contra las pulsaciones. Por
  debajo de $n_q = 40$:

  $$\frac{d_3}{d_2} \ge 1{,}015 + 0{,}08\left(\frac{\rho H}{\rho_{Ref} H_{Ref}} - 0{,}1\right)^{0{,}8}$$

  Nunca baja de 1,015. Por encima de $n_q = 40$ es $1{,}04 + 0{,}001\,(n_q - 40)$.
- **Anchuras:** $b_3 = 1{,}15\,b_2$ y $b_4 = b_3$, en el centro de los márgenes del libro.
- **Diámetro de salida:** se busca con el método de Brent el $d_4$ cuyo canal da
  $L_{3-4}/a_3 = 4{,}5$, en el centro del criterio C. La búsqueda va de $1{,}05\,d_3$ a
  $(1{,}15 + 0{,}01\,n_q)\,d_2$.

### Propiedades de los álabes

**Número de álabes:** 8 por debajo de $n_q = 20$ y 12 por encima. Se descartan los
números con $m = \nu_3 z_{Le} - \nu_2 z_{La} = 0$ para armónicos $\nu < 4$ (tabla 10.2),
porque excitan vibraciones. Si el número preferido no pasa la regla, se toma el más
cercano que sí la pasa.

**Ángulo de entrada** (ecs. T7.7.3 a T7.7.6). Es el del vórtice libre en $d_3$ con el
bloqueo $\tau_3$ de los propios álabes, que depende del ángulo. Se resuelve por punto
fijo:

$$\tan\alpha'_3 = \frac{\tau_3\,c_{3m}}{c_{3u}}, \qquad \alpha_{3B} = \alpha'_3 + i_3$$

**Garganta** (ecs. T7.7.8a y T7.7.10). La anchura que fija el punto de máximo
rendimiento es:

$$X_{Le} = \exp\!\left(\frac{Q_{Le}}{b_3\,c_{2u}\,r_2\,z_{Le}}\right), \qquad
a_3 = f_{a3}\,\frac{d_3}{2}\,(X_{Le} - 1)$$

con $f_{a3} = 1{,}15$, dentro del margen de 1,1 a 1,3.

**Espesores:** $e_3 = 0{,}0125\,d_2$ en el borde de ataque y $e_4 = 1{,}5\,e_3$ en el de
salida. El borde de ataque es elíptico (3:1) y el de salida, cortado.

### Líneas medias

La línea media se traza en el plano por mapeo conforme, $d\theta = dr/(r\tan\alpha)$, con
una ley de ángulo:

$$\alpha(t) = \alpha_{3B} + (\alpha_{4B} - \alpha_{3B})\,t^p, \qquad
t = \frac{r - r_3}{r_4 - r_3}$$

**Exponente $p$:** se elige con el método de Brent, entre 0,5 y 6, para que la garganta
geométrica sea la de la ec. T7.7.10. La garganta geométrica es la menor distancia del
borde de ataque del álabe siguiente a la cara interior de este. Un $p$ mayor mantiene el
álabe más tiempo cerca de $\alpha_{3B}$ y estrecha la garganta. En la bomba de
referencia, un ángulo lineal ($p = 1$) daba una garganta un 15 % más ancha.

**Ángulo de salida:** se itera para que el canal tenga la relación de áreas óptima
(ec. 7.30):

$$A_{R,opt} = 1{,}05 + 0{,}184\,\frac{L_{3-4}}{R_{eq}}, \qquad
R_{eq} = \sqrt{\frac{a_3 b_3}{\pi}}, \qquad a_4 = t_4 \sin\alpha_{4B} - e_4$$

- La apertura equivalente se limita al 90 % de $\vartheta_{al}$ (ecs. 1.44 y 1.45).
- $\alpha_{4B}$ queda entre $\alpha_{3B}$ y 45°.

El flujo sale con el ángulo del álabe, sin desviación, y eso fija $c_{4u}$ para el
componente siguiente.

### Criterios

Son los criterios A a F del capítulo 7.9.1. El informe los muestra todos, y los que
salen de su margen generan aviso.

| | Criterio | Margen |
|---|---|---|
| A | Relación de áreas del canal frente a $A_{R,opt}$; recuperación óptima $c_{p,opt} = 0{,}36\,(L_{3-4}/R_{eq})^{0{,}26}$ (ec. 7.31) | informativo |
| B | Deceleración hasta la garganta, $c_{3q}/c_2$ | 0,7 a 0,85 (hasta 1 con $n_q < 15$) |
| C | Longitud del canal, $L_{3-4}/a_3$ | 2,5 a 6 |
| D | Forma de la garganta, $b_3/a_3$ | 0,8 a 2 |
| E | Energía a la salida, $c_4^2/(2gH)$ | 0,02 a 0,04 en bombas multietapa |
| F | Apertura del canal frente a $\vartheta_{al}$ | por debajo |

También avisan:

- una combinación de álabes con $m = 0$;
- una garganta geométrica que se aleje más de un 15 % de la ec. T7.7.10.

## CAD

Los sólidos se construyen en milímetros con Open CASCADE:

- **Voluta:** cada sección es un contorno cerrado con una arista por tramo suave, para
  que la spline no suavice los quiebros entre flancos y esquinas. El loft recorre la
  espiral y la tobera, y se une al anillo que va de la salida del rodete al círculo de
  la lengüeta. El resultado es el dominio fluido, `volute.<ext>`.
- **Difusor:** el canal entre las paredes laterales se obtiene por revolución, y se le
  restan los álabes, que son prismas entre las dos paredes. Salen el dominio fluido,
  `diffuser-N.<ext>`, y los álabes recortados a las paredes, `diffuser-N-vanes.<ext>`.

## Bomba de referencia

La bomba es de 100 m³/h, 32 m y 2900 rpm, con $n_q = 35{,}9$, 7 álabes, $d_2 = 172{,}2$ mm
y $b_2 = 17{,}4$ mm. Todos los valores son automáticos.

**Voluta tras el rodete:**

| Magnitud | Valor |
|---|---|
| $d_z$ | 193,3 mm ($1{,}122\,d_2$, el mínimo de la tabla 10.2) |
| $e_3$ / $b_3$ | 3,45 mm / 27,6 mm |
| $\alpha'_3 = \alpha_{3B}$ | 8,0° |
| Garganta | 3123 mm², 53,5 mm de alto, $r_a$ = 151,8 mm |
| $\Delta a_3$ | 0,044 mm |
| $c_{3q}$, $c_{3q}/c_2$, $B/H$ | 8,89 m/s, 0,65, 1,67 |
| Tobera | $L$ = 126 mm, $d_d$ = 94,1 mm, $A_R$ = 2,23 |
| Apertura de la tobera | 7,0° (admisible 8,25°) |
| $c_{p,id}$ | 0,80 |

**Difusor con álabes entre rodete y voluta:**

| Magnitud | Valor |
|---|---|
| $d_3$ / $d_4$ | 174,8 mm ($1{,}015\,d_2$) / 230,8 mm ($1{,}340\,d_2$) |
| $b_3 = b_4$ | 20,1 mm |
| $z_{Le}$ | 12 (con $z_{La} = 7$ no hay $m = 0$) |
| $\alpha'_3 = \alpha_{3B}$ / $\alpha_{4B}$ | 13,6° / 25,5° |
| $a_3$ | 10,71 mm, igual a la ec. T7.7.10 |
| $a_4$ / $L_{3-4}$ | 22,8 mm / 48,2 mm |
| A | $A_R$ = 2,12 (óptimo 2,12, $c_{p,opt}$ = 0,57) |
| B | 0,79 |
| C | 4,5 |
| D | 1,87 |
| E | 0,041 |
| F | 4,5° (admisible 6,8°) |
| Salida | $c_{4u}$ = 4,58 m/s, $c_{4m}$ = 2,18 m/s |

Tras el difusor, la voluta recibe mucho menos momento angular: $c_u r$ baja de 1,14 a
0,53 m²/s. Con la misma ley sale más grande: garganta de 10 147 mm² y $d_z$ = 259 mm.

**Difusor sin álabes:**

| Magnitud | Valor |
|---|---|
| $d_3$ / $d_4$ | 172,2 mm / 258,4 mm |
| $b_3$ | 18,3 mm |
| $\alpha$ | 12,0° en todo el anillo |
| $c$ de entrada / de salida | 13,5 m/s / 9,0 m/s |
| $c_{p,id}$ | 0,56 |
| Recorrido | 110° de envolvimiento, 208 mm de longitud |

## Limitaciones

- **Volutas:** no hay volutas gemelas (con dos descargas), volutas de entrada ni voluta
  detrás de otra voluta. En la voluta doble, el nervio acaba en la entrada de la
  descarga, y el reparto de caudal entre los dos canales según sus resistencias
  (Gülich tabla 1.5) no se calcula: cada uno lleva la parte de su ángulo.
- **Multietapa:** no hay canales de retorno.
- **Secciones:** solo rectangulares, trapezoidales y circulares, simétricas. Faltan las
  de Bézier, las asimétricas y las externas.
- **Áreas de la espiral:** faltan las reglas de Pfleiderer y Stepanoff.
- **Tobera:** solo es recta, no curva ni tangencial.
- **Lengüeta:** no se puede elegir entre redondeada y arista viva.
- **Sólidos:** el CAD da el dominio fluido, no la carcasa con su espesor de pared.
- **Flujo:**
  - el difusor sin álabes no tiene rozamiento;
  - en el difusor con álabes el flujo sale sin desviación.
