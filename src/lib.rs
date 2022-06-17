pub use futures;
pub use paste::paste;
use std::future::Future;

trait Generator {
    type Fut: Future;

    fn gen(&self) -> Self::Fut;
}

impl<Fut, T> Generator for T
where
    Fut: Future,
    T: Fn() -> Fut,
{
    type Fut = Fut;

    fn gen(&self) -> Self::Fut {
        (self)()
    }
}

#[macro_export]
macro_rules! select_loop {
    // Uses a declarative macro to do **most** of the work. While it is possible
    // to implement fully with a declarative macro, a procedural macro is used
    // to enable improved error messages.
    //
    // The macro is structured as a tt-muncher. All branches are processed and
    // normalized. Once the input is normalized, it is passed to the top-most
    // rule. When entering the macro, `@{ }` is inserted at the front. This is
    // used to collect the normalized input.
    //
    // The macro only recurses once per branch. This allows using `select_loop!`
    // without requiring the user to increase the recursion limit.

    // All input is normalized, now transform.
    (@ {
        ( $($skip:tt)+ )
        // Normalized select_loop branches. `( $skip )` is a set of `_` characters.
        // There is one `_` for each select_loop branch **before** this one. Given
        // that all input futures are stored in a tuple, $skip is useful for
        // generating a pattern to reference the future for the current branch.
        // $skip is also used as an argument to `count!`, returning the index of
        // the current select_loop branch.
        $( ( $($variant:tt)+ ) $bind:pat = $gen:expr, if $c:expr => $handle:expr, )+
    }) => {{
        // Create a scope to separate polling from handling the output. This
        // adds borrow checker flexibility when using the macro.
        {
            $crate::paste!{
                enum ReturnValue<$([<$($variant)+>]),+> {
                    $([<$($variant)+>]([<$($variant)+>]),)+
                }
            }

            $crate::paste!{
                struct RecursiveGenerator<'a, $([<$($variant)+>]),+> {
                    inner: ::std::boxed::Box<
                        dyn $crate::Generator<
                            Fut = $crate::futures::future::BoxFuture<'a, ReturnValue<$([<$($variant)+>]),+>>
                        >
                        + Send + Sync + 'a
                    >,
                }
            }

            $crate::paste!{
                async fn generate<'a, $([<$($variant)+>]),+>(
                    generator: RecursiveGenerator<'a,  $([<$($variant)+>]),+>
                ) -> (RecursiveGenerator<'a, $([<$($variant)+>]),+>, ReturnValue<$([<$($variant)+>]),+>) {
                    $crate::futures::FutureExt::map(generator.inner.gen(), |v| (generator, v)).await
                }
            }

            let mut __futures = Vec::new();
            $(
                $crate::paste!{
                    let __inner: ::std::boxed::Box<dyn $crate::Generator<Fut = _> + Send + Sync> = ::std::boxed::Box::new(|| {
                        $crate::futures::future::FutureExt::boxed(
                            $crate::futures::FutureExt::map(
                                ($gen)(), ReturnValue::[<$($variant)+>]
                            )
                        )
                    });
                }
                let __generator = RecursiveGenerator {
                    inner: __inner,
                };
                __futures.push(generate(__generator));
            )+

            let mut stream = <$crate::futures::stream::FuturesUnordered<_> as ::std::iter::FromIterator<_>>::from_iter(__futures);
            loop {
                if let Some((generator, v)) = $crate::futures::StreamExt::next(&mut stream).await {
                    $crate::paste!{
                        match v {
                            $(ReturnValue::[<$($variant)+>](__v) => if let $bind = __v {
                                if $c {
                                    $handle
                                }
                            }),+
                        }
                        stream.push(generate(generator));
                    }
                }
            }
        };
    }};

    // ==== Normalize =====

    // These rules match a single `select_loop!` branch and normalize it for
    // processing by the first rule.
    (@ { ( $($last:tt)+ ) $($t:tt)* } $p:pat = $f:expr, if $c:expr => $h:block, $($r:tt)* ) => {
        $crate::select_loop!(@{ ($($last)+A) $($t)* ($($last)+) $p = $f, if $c => $h, } $($r)*)
    };
    (@ { ( $($last:tt)+ ) $($t:tt)* } $p:pat = $f:expr => $h:block, $($r:tt)* ) => {
        $crate::select_loop!(@{ ($($last)+A) $($t)* ($($last)+) $p = $f, if true => $h, } $($r)*)
    };
    (@ { ( $($last:tt)+ ) $($t:tt)* } $p:pat = $f:expr, if $c:expr => $h:block $($r:tt)* ) => {
        $crate::select_loop!(@{ ($($last)+A) $($t)* ($($last)+) $p = $f, if $c => $h, } $($r)*)
    };
    (@ { ( $($last:tt)+ ) $($t:tt)* } $p:pat = $f:expr => $h:block $($r:tt)* ) => {
        $crate::select_loop!(@{ ($($last)+A) $($t)* ($($last)+) $p = $f, if true => $h, } $($r)*)
    };
    (@ { ( $($last:tt)+ ) $($t:tt)* } $p:pat = $f:expr, if $c:expr => $h:expr ) => {
        $crate::select_loop!(@{ ($($last)+A) $($t)* ($($last)+) $p = $f, if $c => $h, })
    };
    (@ { ( $($last:tt)+ ) $($t:tt)* } $p:pat = $f:expr => $h:expr ) => {
        $crate::select_loop!(@{ ($($last)+A) $($t)* ($($last)+) $p = $f, if true => $h, })
    };
    (@ { ( $($last:tt)+ ) $($t:tt)* } $p:pat = $f:expr, if $c:expr => $h:expr, $($r:tt)* ) => {
        $crate::select_loop!(@{ ($($last)+A) $($t)* ($($last)+) $p = $f, if $c => $h, } $($r)*)
    };
    (@ { ( $($last:tt)+ ) $($t:tt)* } $p:pat = $f:expr => $h:expr, $($r:tt)* ) => {
        $crate::select_loop!(@{ ($($last)+A) $($t)* ($($last)+) $p = $f, if true => $h, } $($r)*)
    };

    // ===== Entry point =====

    ($p:pat = $($t:tt)* ) => {
        $crate::select_loop!(@{ (A) } $p = $($t)*)
    };

    () => {
        compile_error!("select_loop! requires at least one branch.")
    };
}

#[cfg(test)]
mod test {
    use super::select_loop;

    #[tokio::test]
    async fn it_works() {
        async fn succ(s: &str) -> Result<(), ()> {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            println!("succ({})", s);
            Ok(())
        }

        async fn fail1() -> Result<i32, ()> {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            println!("fail");
            Err(())
        }

        async fn fail2() -> Result<(), ()> {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            println!("fail");
            Err(())
        }

        let s = "abc".to_string();
        let mut modify = "hello".to_string();

        let value = select_loop! {
            Ok(v) = || succ(&s), if true => {
                modify.push_str("a");
                println!("succ passed with {:?}", v);
            }
            Ok(v) = fail1, if v > 0 => {
                modify.push_str("b");
                return ();
            }
            Err(e) = fail2 => {
                modify.push_str("c");
                break e;
            }
        };
        println!("Got {:?}, modify = {}", value, modify);
    }
}
