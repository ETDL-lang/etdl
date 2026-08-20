//! `cargo run -p etdl-probability-core --example distributions`
//!
//! The five foundational distributions: Bernoulli, Binomial, Beta,
//! Exponential, Normal.

use etdl_probability_core::distribution::{Bernoulli, Beta, Binomial, Exponential, Normal};
use etdl_probability_core::Probability;

fn main() {
    println!("== Bernoulli(0.3) ==");
    let bern = Bernoulli::new(Probability::new(0.3).unwrap());
    println!("  P(X=1) = {}", bern.pmf(1).unwrap());
    println!("  P(X=0) = {}", bern.pmf(0).unwrap());
    println!("  mean = {}, variance = {}", bern.mean(), bern.variance());

    println!("== Binomial(100000, 0.00037) ==");
    let binom = Binomial::new(100_000, Probability::new(0.00037).unwrap()).unwrap();
    println!("  E[X] = {} (expected failures)", binom.mean());
    println!("  P(X <= 37) = {:.6}", binom.cdf(37).value());
    println!("  P(X = 37)  = {:.6}", binom.pmf(37).value());

    println!("== Beta(3, 9) (a Beta-Binomial posterior) ==");
    let beta = Beta::new(3.0, 9.0).unwrap();
    println!("  mean = {}", beta.mean());
    println!("  variance = {}", beta.variance());
    println!(
        "  95% credible interval = [{:.4}, {:.4}]",
        beta.quantile(0.025),
        beta.quantile(0.975)
    );

    println!("== Exponential(0.001) (rate per hour) ==");
    let exp = Exponential::new(0.001).unwrap();
    println!("  mean time to event = {} hours", exp.mean());
    println!("  P(event by 500h)    = {:.6}", exp.cdf(500.0));
    println!("  P(survives past 500h) = {:.6}", exp.survival(500.0));

    println!("== Normal(0.0024, 0.0003) (an uncertain probability estimate) ==");
    let normal = Normal::new(0.0024, 0.0003).unwrap();
    println!(
        "  95% interval = [{:.6}, {:.6}]",
        normal.quantile(0.025),
        normal.quantile(0.975)
    );
    println!("  P(X <= 0.003) = {:.6}", normal.cdf(0.003));
}
