# Shutdown
Library to simplify signal monitoring to interrupt your process.

```
    extern crate shutdown;

    use shutdown::prelude::*;

    fn main() {
        let core = tokio_core::reactor::Core::new();
        
        //use a callback to receive notification when done
        shutdown::SignalHandler::watch(|signal| {
            println!("Signal invoked {:?}");
        });

        //create a future to wait for
        let signal_received = shutdown::SignalHandler::new();
        
        core.run(signal_received).expect("Error handling shutdown");

        println!("All done!");
    }
```
