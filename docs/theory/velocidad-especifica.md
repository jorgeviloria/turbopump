# Velocidad específica

Implementación: [`crates/tp-units/src/specific_speed.rs`](../../crates/tp-units/src/specific_speed.rs)

## Para qué sirve

La velocidad específica clasifica una turbomáquina por la forma de su rodete, sin importar
su tamaño:

- **Valores bajos:** rodetes radiales, estrechos y de gran diámetro.
- **Valores altos:** rodetes de flujo mixto y axiales.

Casi todas las correlaciones empíricas del diseño preliminar se expresan en función de la
velocidad específica: eficiencia, coeficiente de presión, relaciones de diámetro, etc.

## Definiciones

Todas describen la misma magnitud con distintas unidades. Turbopump guarda solo la forma
adimensional y calcula las demás a partir de ella.

**Forma adimensional:** también es el "número de tipo" K de la norma ISO 2548.

$$\omega_s = \frac{\omega \sqrt{Q}}{Y^{3/4}}, \qquad Y = g\,H$$

Unidades: ω en rad/s, Q en m³/s, Y en J/kg.

**nq europea:** la usual en bombas.

$$n_q = \frac{n \sqrt{Q}}{H^{3/4}}$$

Unidades: n en rpm, Q en m³/s, H en m.

**Ns de EE. UU.:** la misma fórmula, con n en rpm, Q en galones US por minuto y H en pies.

**σ de Cordier:**

$$\sigma = \frac{\varphi^{1/2}}{\psi^{3/4}}, \qquad \varphi = \frac{Q}{\tfrac{\pi}{4} D^2 u}, \qquad \psi = \frac{2Y}{u^2}$$

Si se sustituye u = πDn en estas definiciones, se obtiene σ = ω_s / (√π · 2^{3/4}).

## Factores de conversión

| Relación | Valor | Cómo se obtiene |
|---|---|---|
| nq / ω_s | 52,92 | g^{3/4} · 60 / 2π |
| Ns US / nq | 51,65 | √15 850,3 · 0,3048^{3/4} |
| nq / σ | 157,8 | 52,92 · √π · 2^{3/4} |

Los tests de `specific_speed.rs` comprueban estos tres factores.

## Convenciones

- **Rodete de doble aspiración:** Q es el caudal **por boca**, la mitad del caudal total
  (`--eyes 2` en el CLI).
- **Bomba multietapa:** H es la altura **por etapa** (`--stages N`).

## Referencias

- J. F. Gülich, *Centrifugal Pumps*, 2.ª ed., Springer 2010, §3.4.
- O. Cordier, "Ähnlichkeitsbedingungen für Strömungsmaschinen", *Brennstoff-Wärme-Kraft*
  5 (1953).
- S. L. Dixon, C. A. Hall, *Fluid Mechanics and Thermodynamics of Turbomachinery*,
  Butterworth-Heinemann, cap. 2.
- ISO 2548, *Centrifugal, mixed flow and axial pumps — Code for acceptance tests*.
