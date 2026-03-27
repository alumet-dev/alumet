pub mod country;
pub mod user_override;
pub mod world_avg;

/// Trait defining a common interface for emission intensity providers.
///
/// Three implementations are available, selected via plugin configuration:
/// - [`CountryIntensityProvider`]  — looks up the intensity for a specific country
/// - [`OverrideIntensityProvider`] — uses a fixed value supplied by the user
/// - [`WorldAvgIntensityProvider`] — falls back to the world average intensity
pub trait EmissionIntensityProvider: Send {
    /// Returns the current carbon emission intensity in gCO₂/Wh.  
    fn get_intensity(&self) -> anyhow::Result<f64>;

    /// Returns whether the intensity is static (constant) or dynamic (may change over time).
    ///
    /// If `true`, [`get_intensity`] is called once at initialization and cached.
    /// If `false`, it is called once per measurement batch.
    fn is_intensity_static(&self) -> bool;
}
