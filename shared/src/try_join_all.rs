#[macro_export]
macro_rules! try_join_all {
    ($(
        let $var:pat = $e:expr;
    )*) => {
        let ($( $var ),*) = tokio::try_join!($( async { $e } ),*)?;
    };
}
