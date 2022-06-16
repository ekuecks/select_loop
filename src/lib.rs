use std::future::Future;
pub use futures;

trait Generator {
    type Fut: Future;

    fn gen(&self) -> Self::Fut;
}

impl<Fut, T> Generator for T where Fut: Future, T: Fn() -> Fut {
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
        ( $($count:tt)* )
        // Normalized select_loop branches. `( $skip )` is a set of `_` characters.
        // There is one `_` for each select_loop branch **before** this one. Given
        // that all input futures are stored in a tuple, $skip is useful for
        // generating a pattern to reference the future for the current branch.
        // $skip is also used as an argument to `count!`, returning the index of
        // the current select_loop branch.
        $( ( $($skip:tt)* ) $bind:pat = $gen:expr, if $c:expr => $handle:expr, )+
    }) => {{
        // Create a scope to separate polling from handling the output. This
        // adds borrow checker flexibility when using the macro.
        {
            use $crate::Generator;
            use ::std::iter::FromIterator;
            use $crate::futures::future::FutureExt;
            use $crate::futures::StreamExt;

            enum __ControlFlow<C, B> {
                Continue(C),
                Break(B),
            }

            struct RecursiveGenerator<'a, T> {
                inner: ::std::boxed::Box<dyn Generator<Fut = $crate::futures::future::BoxFuture<'a, __ControlFlow<(), T>>> + Send + Sync + 'a>,
            }

            async fn generate<'a, T>(generator: RecursiveGenerator<'a, T>) -> __ControlFlow<RecursiveGenerator<'a, T>, T> {
                generator.inner.gen().map(|v| match v {
                    __ControlFlow::Continue(()) => __ControlFlow::Continue(generator),
                    __ControlFlow::Break(v) => __ControlFlow::Break(v),
                }).await
            }

            let mut __futures = Vec::new();
            $(
                let __inner: ::std::boxed::Box<dyn Generator<Fut = _> + Send + Sync> = ::std::boxed::Box::new(|| {
                    $crate::futures::future::FutureExt::boxed($gen().then(|__v| {
                        #[allow(unreachable_code, unused_variables)]
                        async move {
                            let value = loop {
                                if let $bind = __v {
                                    if $c {
                                        $handle
                                    }
                                }
                                return __ControlFlow::Continue(());
                            };
                            return __ControlFlow::Break(value);
                        }
                }))
                });
                let __generator = RecursiveGenerator {
                    inner: __inner,
                };
                __futures.push(generate(__generator));
            )+

            let mut stream = $crate::futures::stream::FuturesUnordered::from_iter(__futures);
            loop {
                if let Some(flow) = stream.next().await {
                    match flow {
                        __ControlFlow::Continue(generator) => {
                            stream.push(generate(generator));
                        },
                        __ControlFlow::Break(v) => break v,
                    }
                }
            }
        };
    }};

    // ==== Normalize =====

    // These rules match a single `select_loop!` branch and normalize it for
    // processing by the first rule.
    (@ { ( $($s:tt)* ) $($t:tt)* } $p:pat = $f:expr, if $c:expr => $h:block, $($r:tt)* ) => {
        $crate::select_loop!(@{ ($($s)* _) $($t)* ($($s)*) $p = $f, if $c => $h, } $($r)*)
    };
    (@ { ( $($s:tt)* ) $($t:tt)* } $p:pat = $f:expr => $h:block, $($r:tt)* ) => {
        $crate::select_loop!(@{ ($($s)* _) $($t)* ($($s)*) $p = $f, if true => $h, } $($r)*)
    };
    (@ { ( $($s:tt)* ) $($t:tt)* } $p:pat = $f:expr, if $c:expr => $h:block $($r:tt)* ) => {
        $crate::select_loop!(@{ ($($s)* _) $($t)* ($($s)*) $p = $f, if $c => $h, } $($r)*)
    };
    (@ { ( $($s:tt)* ) $($t:tt)* } $p:pat = $f:expr => $h:block $($r:tt)* ) => {
        $crate::select_loop!(@{ ($($s)* _) $($t)* ($($s)*) $p = $f, if true => $h, } $($r)*)
    };
    (@ { ( $($s:tt)* ) $($t:tt)* } $p:pat = $f:expr, if $c:expr => $h:expr ) => {
        $crate::select_loop!(@{ ($($s)* _) $($t)* ($($s)*) $p = $f, if $c => $h, })
    };
    (@ { ( $($s:tt)* ) $($t:tt)* } $p:pat = $f:expr => $h:expr ) => {
        $crate::select_loop!(@{ ($($s)* _) $($t)* ($($s)*) $p = $f, if true => $h, })
    };
    (@ { ( $($s:tt)* ) $($t:tt)* } $p:pat = $f:expr, if $c:expr => $h:expr, $($r:tt)* ) => {
        $crate::select_loop!(@{ ($($s)* _) $($t)* ($($s)*) $p = $f, if $c => $h, } $($r)*)
    };
    (@ { ( $($s:tt)* ) $($t:tt)* } $p:pat = $f:expr => $h:expr, $($r:tt)* ) => {
        $crate::select_loop!(@{ ($($s)* _) $($t)* ($($s)*) $p = $f, if true => $h, } $($r)*)
    };

    // ===== Entry point =====

    ($p:pat = $($t:tt)* ) => {
        $crate::select_loop!(@{ () } $p = $($t)*)
    };

    () => {
        compile_error!("select_loop! requires at least one branch.")
    };
}


#[cfg(test)]
mod tests {
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

        let value = select_loop! {
            Ok(v) = || succ(&s), if true => {
                println!("succ passed with {:?}", v);
            }
            Ok(v) = fail1, if v > 0 => {
                panic!("Impossible: {:?}", v);
            }
            Err(e) = fail2 => {
                break e;
            }
        };
        println!("Got {:?}", value);
    }
}
